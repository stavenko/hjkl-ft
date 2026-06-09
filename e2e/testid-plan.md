# data-testid Migration Plan

## Why
Tests use Russian i18n text to find elements. Changing language breaks all tests.
Must use `data-testid` attributes instead.

## Naming Convention
- `{feature}-btn-{action}` — buttons
- `{feature}-input-{field}` — inputs
- `{feature}-select-{field}` — dropdowns
- `{feature}-link-{target}` — nav links
- `{feature}-checkbox-{name}` — checkboxes

## Elements to tag (by file)

### app.rs
- nav-diary, nav-recipes, nav-settings
- banner-session-expired, banner-session-expiring, btn-session-renew

### auth_page.rs
- auth-btn-register, auth-btn-login
- auth-btn-show-qr, auth-btn-scan-qr, auth-btn-try-passkey, auth-btn-back
- auth-btn-copy-link, auth-qr-display

### pwa_prompt.rs
- pwa-btn-dismiss

### pair_page.rs (PairPageNew)
- pair-new-btn-scan, pair-new-btn-show, pair-new-btn-back
- pair-new-btn-copy-link, pair-new-qr-display

### pair_page.rs (PairPageLoggedIn)
- pair-logged-btn-show, pair-logged-btn-scan, pair-logged-btn-back
- pair-logged-btn-copy-link, pair-logged-qr-display

### settings.rs
- settings-checkbox-calories, settings-checkbox-protein, settings-checkbox-fat, settings-checkbox-carbs
- settings-btn-lang-ru, settings-btn-lang-en
- settings-btn-add-device, settings-btn-wipe-all
- settings-btn-add-goal, settings-input-new-nutrient

### diary.rs
- diary-btn-prev-date, diary-btn-date, diary-btn-next-date
- diary-btn-add

### recipes.rs
- recipes-btn-new, recipes-input-search

### qr_scanner.rs
- qr-scanner-btn-cancel, qr-scanner-btn-paste
