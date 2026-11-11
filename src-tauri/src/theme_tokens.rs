//! Native consumers read the same semantic colors as the web UI.
const CSS: &str = include_str!("../../src/styles.css");

pub fn color(name: &str, dark: bool) -> Result<[f64; 4], String> {
    parse(CSS, name, dark)
}

pub fn color_with_accent(
    name: &str,
    dark: bool,
    accent: &crate::settings::AccentColor,
) -> Result<[f64; 4], String> {
    if let Some(hex) = accent
        .preset()
        .and_then(|preset| preset[if dark { "dark" } else { "light" }].get(format!("--{name}")))
        .and_then(serde_json::Value::as_str)
    {
        let mut rgba = [0.0, 0.0, 0.0, 1.0];
        for (i, channel) in rgba[..3].iter_mut().enumerate() {
            *channel = u8::from_str_radix(&hex[1 + i * 2..3 + i * 2], 16)
                .map_err(|_| "Invalid generated accent")? as f64
                / 255.0;
        }
        return Ok(rgba);
    }
    color(name, dark)
}

fn parse(css: &str, name: &str, dark: bool) -> Result<[f64; 4], String> {
    let block = css
        .split_once(if dark { ".dark {" } else { ":root {" })
        .and_then(|(_, tail)| tail.split_once('}'))
        .map(|(block, _)| block)
        .ok_or("Theme block missing")?;
    let prefix = format!("--{name}: oklch(");
    let value = block
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
        .and_then(|tail| tail.split_once(')'))
        .map(|(value, _)| value)
        .ok_or_else(|| format!("Theme token missing: {name}"))?;
    let (channels, alpha) = value.split_once('/').unwrap_or((value, "1"));
    let values = channels
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("Invalid theme token: {name}"))?;
    if values.len() != 3 || !values.iter().all(|value| value.is_finite()) {
        return Err(format!("Invalid theme channels: {name}"));
    }
    let (l, a, b) = (
        values[0],
        values[1] * values[2].to_radians().cos(),
        values[1] * values[2].to_radians().sin(),
    );
    let ll = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let mm = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let ss = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    let rgb = [
        4.0767416621 * ll - 3.3077115913 * mm + 0.2309699292 * ss,
        -1.2684380046 * ll + 2.6097574011 * mm - 0.3413193965 * ss,
        -0.0041960863 * ll - 0.7034186147 * mm + 1.7076147010 * ss,
    ]
    .map(|c| {
        if c <= 0.0031308 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    })
    .map(|c| c.clamp(0.0, 1.0));
    let alpha = alpha.trim();
    let opacity = alpha
        .trim_end_matches('%')
        .parse::<f64>()
        .map_err(|_| format!("Invalid theme alpha: {name}"))?
        / if alpha.ends_with('%') { 100.0 } else { 1.0 };
    if !opacity.is_finite() {
        return Err(format!("Invalid theme alpha: {name}"));
    }
    Ok([rgb[0], rgb[1], rgb[2], opacity.clamp(0.0, 1.0)])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_accent_tracks_theme_and_choice_without_tinting_background() {
        let sky = crate::settings::AccentColor::default();
        let rose = crate::settings::AccentColor {
            family: "rose".into(),
            shade: 500,
        };
        for dark in [false, true] {
            let recording = color_with_accent("overlay-recording", dark, &sky).unwrap();
            assert_eq!(
                recording,
                color_with_accent("overlay-status", dark, &sky).unwrap()
            );
            assert_ne!(
                recording,
                color_with_accent("overlay-recording", dark, &rose).unwrap()
            );
            assert_eq!(
                color_with_accent("overlay-bg", dark, &rose),
                color("overlay-bg", dark)
            );
            assert_eq!(recording[3], 1.0);
            assert!(color_with_accent("missing", dark, &sky).is_err());
        }
        assert_ne!(
            color_with_accent("overlay-status", false, &sky),
            color_with_accent("overlay-status", true, &sky)
        );
    }

    #[test]
    fn semantic_colors_match_web_theme_and_alpha() {
        for dark in [false, true] {
            for name in [
                "sidebar",
                "foreground",
                "border",
                "overlay-bg",
                "overlay-fg",
            ] {
                assert!(color(name, dark)
                    .unwrap()
                    .iter()
                    .all(|c| (0.0..=1.0).contains(c)));
            }
        }
        assert!(color("missing", true).is_err());
        let white = parse(":root {\n--test: oklch(1 0 0 / 25%);\n}", "test", false).unwrap();
        assert!(white[..3]
            .iter()
            .all(|channel| (channel - 1.).abs() < 1e-12));
        assert_eq!(white[3], 0.25);
    }
    #[test]
    fn malformed_channels_and_nonfinite_values_are_rejected() {
        for value in ["1 0", "NaN 0 0", "1 0 0 / NaN", "bad 0 0"] {
            assert!(parse(
                &format!(":root {{\n--test: oklch({value});\n}}"),
                "test",
                false
            )
            .is_err());
        }
    }
}
