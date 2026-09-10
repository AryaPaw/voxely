---
description: Find evidence-backed product, architecture, UX, legacy, and quality improvements without changing code
---

# Improve Command

Проведи независимый read-only аудит текущего проекта и предложи улучшения, которые действительно повышают качество продукта.

## Usage

```text
/improve
/improve ui
/improve architecture
/improve tests
/improve legacy
/improve <scope> --deep
```

`$ARGUMENTS` задаёт область. Без аргументов проверяй весь текущий проект. `--deep` включает несколько независимых audit tracks и adversarial review.

## Contract

- По умолчанию ничего не изменяй, не устанавливай, не запускай серверы, не коммить и не создавай задачи во внешних сервисах
- Сначала изучи применимые rules, Git status, manifests, configs, runtime owners, tests и актуальную документацию
- Сохраняй unrelated dirty work и не считай старые планы, отчёты или комментарии сильнее текущего кода
- Следуй активной model-routing policy; перед каждым делегированием явно выбирай разрешённую модель, effort и Fast state
- Не предлагай legacy compatibility для private broken code. Совместимость persisted user data и public contracts оценивай отдельно
- Не называй идею дефектом без конкретного evidence path

## Audit Tracks

Проверь применимые направления:

1. Пользовательские дефекты и несогласованное поведение
2. Мёртвый, obsolete, legacy и дублирующий код
3. Архитектурный drift, конкурирующие источники правды и неверные зависимости
4. UX, accessibility, дизайн-токены, responsive и platform fidelity
5. Ошибки, silent fallbacks, concurrency, lifecycle и data integrity
6. Security, privacy, performance и dependency risks
7. False-green тесты, coverage gaps, отсутствующая runtime/E2E проверка
8. Недостающие product details, observability, delivery и maintainability

Для `--deep` делегируй только независимые области. Reviewer не должен изменять код. После первичного аудита отдельно оспорь все CRITICAL/HIGH выводы.

## Evidence Standard

Каждый вывод обязан содержать:

- `CONFIRMED`, `HYPOTHESIS` или `PRODUCT IDEA`
- severity: `CRITICAL`, `HIGH`, `MEDIUM` или `LOW`
- точный файл, symbol или runtime flow
- наблюдаемую проблему и её влияние
- правильную remediation boundary
- риск изменения
- проверку, которая докажет исправление

Удаляй выводы, которые не пережили adversarial review. Не заполняй ответ общими best practices.

## Output

Начни с пяти самых ценных действий. Затем выдай:

### Problems

Только подтверждённые дефекты и архитектурные причины.

### Legacy and removal candidates

Что можно удалить, почему это больше не является source of truth и чем подтверждено отсутствие нужных consumers.

### Product ideas

Новые возможности или polish, явно отделённые от исправлений. Для каждой укажи ценность, стоимость и риск.

### Recommended order

Dependency-ordered waves: root causes раньше визуальных симптомов. Для каждой волны дай acceptance criteria и минимальный verification set.

### Not verified

Что требует живого приложения, production data, внешнего доступа или решения пользователя.

Заверши вопросом: какие из предложений добавить в план. Не реализуй их автоматически.
