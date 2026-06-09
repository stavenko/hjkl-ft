export const meta = {
  name: 'testid-migration',
  description: 'Add data-testid to all Leptos components, rewrite tests to use getByTestId',
  phases: [
    { title: 'Add testids', detail: 'Add data-testid attrs to all interactive elements in Leptos components' },
    { title: 'Rewrite tests', detail: 'Replace getByText with getByTestId in all e2e tests' },
    { title: 'Verify', detail: 'Run all tests, fix failures' },
  ],
}

const FRONTEND = '/Users/vasilijstavenko/projects/hjkl-ft/frontend/src'
const E2E = '/Users/vasilijstavenko/projects/hjkl-ft/e2e/tests'
const PLAN = '/Users/vasilijstavenko/projects/hjkl-ft/e2e/testid-plan.md'

phase('Add testids')

// In Leptos view! macro, data-testid is added as: attr:data-testid="value"
// Example: <button attr:data-testid="auth-btn-register" ...>"Register"</button>

const frontendFiles = [
  'app.rs',
  'pages/auth_page.rs',
  'pages/pwa_prompt.rs',
  'pages/pair_page.rs',
  'pages/diary.rs',
  'pages/settings.rs',
  'pages/recipes.rs',
  'pages/recipe_detail.rs',
  'components/diary_add_modal.rs',
  'components/qr_scanner.rs',
  'components/qr_code.rs',
]

await pipeline(
  frontendFiles,
  (file) => agent(`
You are adding data-testid attributes to interactive elements in a Leptos 0.6 Rust frontend file.

File: ${FRONTEND}/${file}

Read ${PLAN} for the naming convention and list of testids to add.
Then read the file at ${FRONTEND}/${file}.

In Leptos view! macros, add data-testid like this:
  <button attr:data-testid="my-btn-id" class="..." on:click=...>"Text"</button>
  <input attr:data-testid="my-input-id" type="text" ... />
  <a attr:data-testid="my-link-id" href="..." ...>...</a>
  <div attr:data-testid="my-container-id" ...>...</div>

Rules:
1. Add attr:data-testid to ALL interactive elements: buttons, inputs, links, checkboxes, select dropdowns
2. Add to key non-interactive elements that tests need to find: banners, QR displays, error notifications
3. Use the naming convention from the plan: {feature}-btn-{action}, {feature}-input-{field}, etc.
4. Do NOT change any other attributes, text, or logic
5. For dynamic elements in loops (e.g. recipe items), use format! to include IDs where possible, or use a stable class-based testid

For file-specific guidance:

**app.rs**: Tag nav links (nav-diary, nav-recipes, nav-settings), session banners (banner-session-expired, banner-session-expiring), renew button (btn-session-renew)

**auth_page.rs**: Tag register button (auth-btn-register), login button (auth-btn-login), show QR (auth-btn-show-qr), scan QR (auth-btn-scan-qr), try passkey (auth-btn-try-passkey), back buttons (auth-btn-back), QR display container

**pwa_prompt.rs**: Tag dismiss button (pwa-btn-dismiss)

**pair_page.rs**: Tag all buttons in both PairPageNew and PairPageLoggedIn with pair-new-* and pair-logged-* prefixes

**settings.rs**: Tag checkboxes (settings-checkbox-{nutrient}), language buttons, add device button, wipe data button

**diary.rs**: Tag date nav buttons, add FAB button

**recipes.rs**: Tag new recipe button, search input

**qr_scanner.rs**: Tag cancel button, paste button

IMPORTANT: Actually edit the file. Only add attr:data-testid attributes. Do not change functionality.
After editing, verify the file still compiles conceptually (no syntax errors in the attr placement).
`, { label: `testid:${file}`, phase: 'Add testids' })
)

log('All testids added to frontend')

phase('Rewrite tests')

const testFiles = [
  'onboarding.spec.ts',
  'navigation.spec.ts',
  'pairing.spec.ts',
  'pairing-flow.spec.ts',
  'helpers.ts',
]

await pipeline(
  testFiles,
  (file) => agent(`
You are rewriting a Playwright e2e test file to use data-testid selectors instead of text-based selectors.

File: ${E2E}/${file}

Read ${PLAN} for the mapping of elements to testids.
Then read the file at ${E2E}/${file}.

Replace ALL text-based selectors with testid-based selectors:

BEFORE:
  page.getByText('Зарегистрироваться')
  page.getByText('Войти', { exact: true })
  page.locator('nav a[href="/settings"]')
  page.getByText('Я хочу использовать в браузере')
  page.locator('.button.is-link.is-light', { hasText: 'Подключить устройство' })

AFTER:
  page.getByTestId('auth-btn-register')
  page.getByTestId('auth-btn-login')
  page.getByTestId('nav-settings')
  page.getByTestId('pwa-btn-dismiss')
  page.getByTestId('settings-btn-add-device')

Rules:
1. Replace ALL getByText() with getByTestId() using the correct testid from the plan
2. Replace ALL CSS selectors that target interactive elements with getByTestId()
3. Keep getByText() ONLY for asserting content text (e.g. expect(page.getByText('some content')).toBeVisible() where the TEXT itself is what we're testing)
4. Do NOT change test logic, assertions, or structure
5. For URL assertions (toHaveURL), keep as-is
6. For network interception (waitForResponse, page.on('request')), keep as-is

Mapping reference (Russian → testid):
- 'Зарегистрироваться' → 'auth-btn-register'
- 'Войти' (exact) → 'auth-btn-login'
- 'Я уже пользовался этим приложением' → text assertion only (keep getByText)
- 'Я хочу использовать в браузере' → 'pwa-btn-dismiss'
- 'Показать QR-код' (on auth login) → 'auth-btn-show-qr'
- 'Сканировать QR-код' (on auth login) → 'auth-btn-scan-qr'
- 'Попробовать войти с ключом входа' → 'auth-btn-try-passkey'
- 'Подключить устройство' (in settings) → 'settings-btn-add-device'
- 'Показать QR-код' (in pair logged) → 'pair-logged-btn-show'
- 'Сканировать QR-код' (in pair logged) → 'pair-logged-btn-scan'
- nav a[href="/"] → 'nav-diary'
- nav a[href="/recipes"] → 'nav-recipes'
- nav a[href="/settings"] → 'nav-settings'
- 'Калории' checkbox → 'settings-checkbox-calories'
- 'Рецепты' h1 → keep getByText for content assertion
- 'Настройки' h1 → keep getByText for content assertion
- 'Сессия истекла' → 'banner-session-expired'
- 'Дневник' (nav) → 'nav-diary'

For helpers.ts:
- 'Зарегистрироваться' → 'auth-btn-register'

IMPORTANT: Actually edit the file. Replace selectors. Do not change test logic.
`, { label: `rewrite:${file}`, phase: 'Rewrite tests' })
)

log('All tests rewritten to use testids')

phase('Verify')

await agent(`
Run all e2e tests and fix any failures.

1. First check frontend compiles:
   cargo check -p frontend

2. Build and deploy:
   cd /Users/vasilijstavenko/projects/hjkl-ft/frontend && trunk build --release && wrangler pages deploy dist --project-name hjkl-ft --branch main --commit-dirty=true

3. Run tests:
   cd /Users/vasilijstavenko/projects/hjkl-ft/e2e && npx playwright test --reporter=list --workers=1

4. If tests fail:
   - If element not found by testid → check the Leptos file has the correct attr:data-testid
   - If strict mode violation → testid is duplicated, make it unique
   - Fix and re-run until ALL tests pass

5. When all pass, report the count.
`, { label: 'verify', phase: 'Verify' })

return 'testid migration complete'
