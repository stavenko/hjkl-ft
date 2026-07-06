use rusqlite::Connection;
use std::sync::Mutex;

enum MigrationKind {
    Sql(&'static str),
    Code(fn(&Connection)),
}

struct Migration {
    version: i64,
    name: &'static str,
    kind: MigrationKind,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial schema",
        kind: MigrationKind::Sql(include_str!("../../../migrations/001_initial.sql")),
    },
    Migration {
        version: 2,
        name: "food: add package_weight and archived",
        kind: MigrationKind::Code(migrate_002),
    },
    Migration {
        version: 3,
        name: "goal: add key column",
        kind: MigrationKind::Code(migrate_003),
    },
    Migration {
        version: 4,
        name: "story: progress flags table",
        kind: MigrationKind::Code(migrate_004),
    },
    Migration {
        version: 5,
        name: "food.is_restaurant + diary.waste_grams",
        kind: MigrationKind::Code(migrate_005),
    },
];

fn migrate_005(conn: &Connection) {
    if !has_column(conn, "food", "is_restaurant") {
        conn.execute_batch("ALTER TABLE food ADD COLUMN is_restaurant INTEGER NOT NULL DEFAULT 0")
            .expect("failed to add is_restaurant");
    }
    if !has_column(conn, "diary", "waste_grams") {
        conn.execute_batch("ALTER TABLE diary ADD COLUMN waste_grams REAL NOT NULL DEFAULT 0")
            .expect("failed to add waste_grams");
    }
}

fn migrate_004(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS story (
            key        TEXT PRIMARY KEY,
            value      INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT ''
        )",
    )
    .expect("failed to create story table");
}

fn migrate_002(conn: &Connection) {
    if !has_column(conn, "food", "package_weight") {
        conn.execute_batch("ALTER TABLE food ADD COLUMN package_weight REAL")
            .expect("failed to add package_weight");
    }
    if !has_column(conn, "food", "archived") {
        conn.execute_batch("ALTER TABLE food ADD COLUMN archived INTEGER NOT NULL DEFAULT 0")
            .expect("failed to add archived");
        // Migrate old deleted -> archived
        if has_column(conn, "food", "deleted") {
            conn.execute_batch("UPDATE food SET archived = deleted")
                .expect("failed to migrate deleted to archived");
        }
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_food_archived ON food(archived)")
        .expect("failed to create index");
}

fn migrate_003(conn: &Connection) {
    if !has_column(conn, "goal", "key") {
        conn.execute_batch("ALTER TABLE goal ADD COLUMN key TEXT NOT NULL DEFAULT ''")
            .expect("failed to add key column");
    }
    let mut stmt = conn
        .prepare("SELECT id, nutrient FROM goal WHERE key = ''")
        .expect("failed to query goals");
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("failed to read goals")
        .filter_map(|r| r.ok())
        .collect();
    for (id, nutrient) in rows {
        let key = crate::use_cases::nutrient_key::generate(&nutrient);
        conn.execute(
            "UPDATE goal SET key = ?1 WHERE id = ?2",
            rusqlite::params![key, id],
        )
        .expect("failed to update goal key");
    }
}

fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = conn.prepare(&sql).expect("failed to query table_info");
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("failed to read columns")
        .filter_map(|r| r.ok())
        .collect();
    columns.iter().any(|c| c == column)
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &str) -> Self {
        let conn = Connection::open(path).expect("failed to open database");
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .expect("failed to set pragmas");
        Self {
            conn: Mutex::new(conn),
        }
    }

    pub fn run_migrations(&self) {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            )",
        )
        .expect("failed to create migrations table");

        let current_version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _migrations",
                [],
                |row| row.get(0),
            )
            .expect("failed to read migration version");

        for migration in MIGRATIONS {
            if migration.version > current_version {
                tracing::info!("Applying migration {}: {}", migration.version, migration.name);
                match &migration.kind {
                    MigrationKind::Sql(sql) => {
                        conn.execute_batch(sql)
                            .unwrap_or_else(|e| panic!("migration {} failed: {e}", migration.version));
                    }
                    MigrationKind::Code(f) => {
                        f(&conn);
                    }
                }
                conn.execute(
                    "INSERT INTO _migrations (version, applied_at) VALUES (?1, datetime('now'))",
                    [migration.version],
                )
                .expect("failed to record migration");
            }
        }

        tracing::info!(
            "Database at migration version {}",
            MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
        );
    }

    pub fn with_conn<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Connection) -> R,
    {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }
}
