use leptos::*;
use leptos_router::*;

use crate::pages;
use crate::services::i18n::t;
use crate::services::{auth, db, platform, push, story, update};

#[derive(Clone, Copy, PartialEq)]
enum AppState {
    PwaPrompt,
    Auth,
    PushOnboarding,
    Ready,
}

fn initial_state() -> AppState {
    if platform::needs_pwa_prompt() {
        AppState::PwaPrompt
    } else if auth::get_user_id().is_none() {
        AppState::Auth
    } else if push::needs_push_onboarding() {
        AppState::PushOnboarding
    } else {
        AppState::Ready
    }
}

/// Invisible component mounted inside `<Router>`: when the user opens a story
/// section page it marks that section seen (clearing its "new" dot) and refreshes
/// the nav-icon attention marker. Lives inside the router so `use_location` works.
#[component]
fn RouteWatcher() -> impl IntoView {
    let location = use_location();
    create_effect(move |_| {
        let path = location.pathname.get();
        if story::is_section_route(&path) {
            spawn_local(async move {
                story::mark_section_seen(&path).await;
                story::refresh_attention();
            });
        }
    });
}

#[component]
pub fn App() -> impl IntoView {
    let state = create_rw_signal(initial_state());

    // Keep the nav-icon story marker live: recompute attention whenever data
    // that feeds a section/task unlock changes — so completing a task lights the
    // dot immediately, even when the user is on another page.
    create_effect(move |_| {
        db::version("story").get();
        db::version("weight_entries").get();
        db::version("step_entries").get();
        db::version("diary").get();
        db::version("goals").get();
        db::version("summaries").get();
        db::version("progress_photos").get();
        db::version("recipes").get();
        story::refresh_attention();
    });

    let after_auth = move || {
        if push::needs_push_onboarding() {
            state.set(AppState::PushOnboarding);
        } else {
            state.set(AppState::Ready);
        }
    };

    view! {
        // Overlays
        {move || match state.get() {
            AppState::PwaPrompt => Some(view! {
                <pages::pwa_prompt::PwaPrompt on_dismiss=Callback::new(move |_| {
                    if auth::get_user_id().is_some() {
                        after_auth();
                    } else {
                        state.set(AppState::Auth);
                    }
                }) />
            }.into_view()),

            AppState::Auth => Some(view! {
                <div style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main);">
                    <pages::auth_page::AuthPage on_authenticated=Callback::new(move |_| {
                        after_auth();
                    }) />
                </div>
            }.into_view()),

            AppState::PushOnboarding => Some(view! {
                <div style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main);">
                    <pages::push_onboarding::PushOnboarding on_done=Callback::new(move |_| {
                        state.set(AppState::Ready);
                    }) />
                </div>
            }.into_view()),

            AppState::Ready => None,
        }}

        // Router always mounted
        <Router>
            <RouteWatcher/>
            <div style="padding-bottom: 4.5rem;">
                <div style="padding: 0.75rem;">
                    <Routes>
                        <Route path="/" view=pages::story::StoryPage />
                        <Route path="/story/intro" view=pages::story_intro::StoryIntroPage />
                        <Route path="/story/setup" view=pages::story_setup::StorySetupPage />
                        <Route path="/story/accounting" view=pages::story_accounting::StoryAccountingPage />
                        <Route path="/story/first-food" view=pages::story_first_food::StoryFirstFoodPage />
                        <Route path="/story/activity" view=pages::story_activity::StoryActivityPage />
                        <Route path="/story/cooking" view=pages::story_cooking::StoryCookingPage />
                        <Route path="/story/bones" view=pages::story_bones::StoryBonesPage />
                        <Route path="/story/restaurant" view=pages::story_restaurant::StoryRestaurantPage />
                        <Route path="/story/next" view=pages::story_next::StoryNextPage />
                        <Route path="/story/ch2-mistake" view=pages::story_ch2_mistake::StoryCh2MistakePage />
                        <Route path="/story/ch2-veg" view=pages::story_ch2_veg::StoryCh2VegPage />
                        <Route path="/story/ch2-protein" view=pages::story_ch2_protein::StoryCh2ProteinPage />
                        <Route path="/story/ch2-snack" view=pages::story_ch2_snack::StoryCh2SnackPage />
                        <Route path="/story/ch2-drinks" view=pages::story_ch2_drinks::StoryCh2DrinksPage />
                        <Route path="/story/ch2-meals" view=pages::story_ch2_meals::StoryCh2MealsPage />
                        <Route path="/story/ch2-night" view=pages::story_ch2_night::StoryCh2NightPage />
                        <Route path="/story/ch3-fat" view=pages::story_ch3_fat::StoryCh3FatPage />
                        <Route path="/story/ch3-beauty" view=pages::story_ch3_beauty::StoryCh3BeautyPage />
                        <Route path="/story/ch3-minimum" view=pages::story_ch3_minimum::StoryCh3MinimumPage />
                        <Route path="/story/ch3-lean" view=pages::story_ch3_lean::StoryCh3LeanPage />
                        <Route path="/story/ch3-lifestyle" view=pages::story_ch3_lifestyle::StoryCh3LifestylePage />
                        <Route path="/paywall" view=pages::paywall::PaywallPage />
                        <Route path="/progress" view=pages::progress::ProgressPage />
                        <Route path="/diary" view=pages::diary::DiaryPage />
                        <Route path="/diary/add" view=pages::diary_add::DiaryAddPage />
                        <Route path="/foods" view=pages::foods::FoodsPage />
                        <Route path="/recipes" view=pages::recipes::RecipesPage />
                        <Route path="/recipes/:id" view=pages::recipe_detail::RecipeDetailPage />
                        <Route path="/settings" view=pages::settings::SettingsPage />
                        <Route path="/settings/goals" view=pages::goals::GoalsPage />
                        <Route path="/settings/privacy" view=pages::privacy::PrivacyPage />
                        <Route path="/weight" view=pages::weight::WeightPage />
                        <Route path="/steps" view=pages::steps::StepsPage />
                        <Route path="/chat" view=pages::chat::ChatPage />
                    </Routes>
                </div>
            </div>

            <nav style="position: fixed; bottom: 0.75rem; left: 50%; transform: translateX(-50%); z-index: 40; background: var(--bulma-scheme-main); display: flex; justify-content: space-around; align-items: center; height: 3.5rem; width: min(26rem, calc(100% - 2rem)); border-radius: 1rem; box-shadow: 0 4px 24px rgba(0,0,0,0.15);">
                <a attr:data-testid="nav-story" href="/" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <span style="position: relative; display: inline-flex;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20" />
                        <path d="M9 7h6" />
                        <path d="M9 11h4" />
                    </svg>
                    {move || story::attention_signal().get().then(|| view! {
                        <span attr:data-testid="nav-story-attention-dot"
                            style="position: absolute; top: -1px; right: -2px; width: 9px; height: 9px; border-radius: 50%; background: var(--bulma-danger); border: 1.5px solid var(--bulma-scheme-main);"></span>
                    })}
                    </span>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.story")}</span>
                </a>
                <a attr:data-testid="nav-diary" href="/diary" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <rect x="3" y="4" width="18" height="18" rx="2" />
                        <line x1="3" y1="10" x2="21" y2="10" />
                        <line x1="8" y1="2" x2="8" y2="6" />
                        <line x1="16" y1="2" x2="16" y2="6" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.diary")}</span>
                </a>
                <a attr:data-testid="nav-recipes" href="/recipes" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                        <polyline points="14 2 14 8 20 8" />
                        <line x1="16" y1="13" x2="8" y2="13" />
                        <line x1="16" y1="17" x2="8" y2="17" />
                        <polyline points="10 9 9 9 8 9" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.recipes")}</span>
                </a>
                <a attr:data-testid="nav-settings" href="/settings" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <span style="position: relative; display: inline-flex;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <circle cx="12" cy="12" r="3" />
                        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
                    </svg>
                    {move || update::available().get().then(|| view! {
                        <span attr:data-testid="nav-settings-update-dot"
                            style="position: absolute; top: -1px; right: -2px; width: 9px; height: 9px; border-radius: 50%; background: var(--bulma-danger); border: 1.5px solid var(--bulma-scheme-main);"></span>
                    })}
                    </span>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.settings")}</span>
                </a>
                <a attr:data-testid="nav-support" href="/chat" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.support")}</span>
                </a>
            </nav>
        </Router>
    }
}
