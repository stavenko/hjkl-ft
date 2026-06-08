CREATE TABLE IF NOT EXISTS food (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    kcal            REAL NOT NULL,
    protein         REAL NOT NULL,
    fat             REAL NOT NULL,
    carbs           REAL NOT NULL,
    nutrients_json  TEXT NOT NULL DEFAULT '{}',
    is_recipe       INTEGER NOT NULL DEFAULT 0,
    recipe_id       TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted         INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS recipe (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    notes       TEXT,
    total_grams REAL,
    finalized   INTEGER NOT NULL DEFAULT 0,
    food_id     TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS recipe_ingredient (
    id          TEXT PRIMARY KEY,
    recipe_id   TEXT NOT NULL,
    food_id     TEXT NOT NULL,
    grams       REAL NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS diary (
    id          TEXT PRIMARY KEY,
    food_id     TEXT NOT NULL,
    date        TEXT NOT NULL,
    time        TEXT,
    grams       REAL NOT NULL,
    meal_label  TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS goal (
    id          TEXT PRIMARY KEY,
    nutrient    TEXT NOT NULL,
    direction   TEXT NOT NULL,
    amount      REAL NOT NULL,
    unit        TEXT NOT NULL,
    period      TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_diary_date ON diary(date);
CREATE INDEX IF NOT EXISTS idx_recipe_ingredient_recipe ON recipe_ingredient(recipe_id);
