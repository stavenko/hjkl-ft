//! Кураторы и их клиенты — глобальный singleton-DO (`idFromName("curators")`).
//!
//! Куратор — САМОСТОЯТЕЛЬНАЯ роль, а не разновидность эксперта: регистрируется
//! свободно, без кода и одобрения оператора (тем и отличается от `admins` в
//! `conversation_index_do`). Личность — обычный `sub` из auth-worker, полученный
//! паскеем на кураторском домене; здесь хранится только профиль (имя, язык) и
//! список клиентов.
//!
//! Клиент — СЛОТ у куратора, а не пользователь: он заводится с именем, которое
//! придумал куратор (у худеющего имени в системе нет), живёт с пригласительным
//! кодом и лишь потом, по согласию, привязывается к `user_id`. Слот переживает
//! отвязку — тем же слотом человека приглашают снова.
//!
//! FAIL LOUDLY: любая ошибка storage поднимается наверх; молчаливых заглушек нет.

use serde::Deserialize;
use worker::*;

use crate::types::{ClientRow, CuratorProfile};

#[derive(Debug, Deserialize)]
struct CuratorRow {
    curator_id: String,
    name: Option<String>,
    lang: Option<String>,
    created_at: String,
}

/// Строка слота в поиске по коду/пользователю — вместе с владельцем, которого
/// в `ClientSqlRow` нет (там выборка уже ограничена одним куратором).
#[derive(Debug, Deserialize)]
struct OwnedClientRow {
    id: String,
    curator_id: String,
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClientSqlRow {
    id: String,
    name: String,
    invite_code: String,
    user_id: Option<String>,
    bound_at: Option<String>,
    unbound_at: Option<String>,
    last_report_at: Option<String>,
    request_days: Option<i64>,
    request_at: Option<String>,
}

impl From<ClientSqlRow> for ClientRow {
    fn from(r: ClientSqlRow) -> Self {
        // Пригласительный код отдаётся наружу ТОЛЬКО у непривязанного слота:
        // у привязанного он погашен, и показывать его куратору незачем.
        let bound = r.user_id.is_some();
        ClientRow {
            id: r.id,
            name: r.name,
            invite_code: (!bound).then_some(r.invite_code),
            bound: r.user_id.is_some(),
            bound_at: r.bound_at,
            unbound_at: r.unbound_at,
            last_report_at: r.last_report_at,
            request_days: r.request_days.map(|d| d as u32),
            request_at: r.request_at,
        }
    }
}

impl From<CuratorRow> for CuratorProfile {
    fn from(r: CuratorRow) -> Self {
        CuratorProfile {
            curator_id: r.curator_id,
            name: r.name.unwrap_or_default(),
            lang: r.lang.unwrap_or_else(|| "ru".to_string()),
            created_at: r.created_at,
        }
    }
}

#[durable_object]
pub struct CuratorIndexDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl CuratorIndexDO {
    fn ensure_schema(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        sql.exec(
            "CREATE TABLE IF NOT EXISTS curators (
                curator_id TEXT PRIMARY KEY,
                name       TEXT,
                lang       TEXT,
                created_at TEXT NOT NULL
            )",
            None,
        )?;
        // last_report — кэш ПОСЛЕДНЕГО присланного отчёта, чтобы дашборд куратора
        // открывался сразу, а не перелистывал переписку. request_* — открытый
        // запрос данных (сколько дней попросили и когда).
        sql.exec(
            "CREATE TABLE IF NOT EXISTS clients (
                id             TEXT PRIMARY KEY,
                curator_id     TEXT NOT NULL,
                name           TEXT NOT NULL,
                invite_code    TEXT NOT NULL UNIQUE,
                user_id        TEXT,
                bound_at       TEXT,
                unbound_at     TEXT,
                last_report_at TEXT,
                last_report    TEXT,
                request_days   INTEGER,
                request_at     TEXT,
                created_at     TEXT NOT NULL
            )",
            None,
        )?;
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_clients_curator ON clients(curator_id)",
            None,
        )?;
        // Один худеющий — один куратор. Ограничение ЧАСТИЧНОЕ: отвязанные слоты
        // держат user_id = NULL и в него не попадают, поэтому один и тот же
        // человек может побывать у многих кураторов, но не у двух разом.
        sql.exec(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_clients_bound_user
             ON clients(user_id) WHERE user_id IS NOT NULL",
            None,
        )?;
        Ok(())
    }

    fn curator(&self, curator_id: &str) -> Result<Option<CuratorRow>> {
        let sql = self.state.storage().sql();
        let rows: Vec<CuratorRow> = sql
            .exec(
                "SELECT curator_id, name, lang, created_at FROM curators WHERE curator_id = ?",
                vec![curator_id.into()],
            )?
            .to_array()?;
        Ok(rows.into_iter().next())
    }

    /// Свободная регистрация: идемпотентна — повторный вызов возвращает уже
    /// заведённый профиль, а не заводит второй и не затирает имя.
    fn handle_register(&self, curator_id: &str) -> Result<Response> {
        if let Some(row) = self.curator(curator_id)? {
            return Response::from_json(&serde_json::json!({
                "created": false, "curator": CuratorProfile::from(row),
            }));
        }
        let sql = self.state.storage().sql();
        let now = now_rfc3339();
        sql.exec(
            "INSERT INTO curators(curator_id,name,lang,created_at) VALUES (?,?,?,?)",
            vec![curator_id.into(), "".into(), "ru".into(), now.as_str().into()],
        )?;
        let row = self
            .curator(curator_id)?
            .ok_or_else(|| Error::RustError("curator vanished right after insert".into()))?;
        Response::from_json(&serde_json::json!({
            "created": true, "curator": CuratorProfile::from(row),
        }))
    }

    fn handle_get(&self, curator_id: &str) -> Result<Response> {
        match self.curator(curator_id)? {
            Some(row) => Response::from_json(&serde_json::json!({
                "found": true, "curator": CuratorProfile::from(row),
            })),
            None => Response::from_json(&serde_json::json!({ "found": false })),
        }
    }

    /// Профиль куратора: имя и язык. Оба поля необязательны — приходит то, что
    /// человек менял, остальное остаётся как было.
    fn handle_set(
        &self,
        curator_id: &str,
        name: Option<&str>,
        lang: Option<&str>,
    ) -> Result<Response> {
        if self.curator(curator_id)?.is_none() {
            return Response::error("curator not found", 404);
        }
        let sql = self.state.storage().sql();
        if let Some(name) = name {
            sql.exec(
                "UPDATE curators SET name = ? WHERE curator_id = ?",
                vec![name.into(), curator_id.into()],
            )?;
        }
        if let Some(lang) = lang {
            sql.exec(
                "UPDATE curators SET lang = ? WHERE curator_id = ?",
                vec![lang.into(), curator_id.into()],
            )?;
        }
        let row = self
            .curator(curator_id)?
            .ok_or_else(|| Error::RustError("curator vanished mid-update".into()))?;
        Response::from_json(&serde_json::json!({ "curator": CuratorProfile::from(row) }))
    }

    fn handle_client_create(&self, curator_id: &str, name: &str) -> Result<Response> {
        if self.curator(curator_id)?.is_none() {
            return Response::error("curator not found", 404);
        }
        let sql = self.state.storage().sql();
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        // UNIQUE(invite_code) — последняя линия обороны, а не расчёт на удачу:
        // при коллизии код перегенерируется, и лишь после восьми попыток мы
        // сдаёмся громко.
        let mut code = generate_invite_code()?;
        for _ in 0..8 {
            match sql.exec(
                "INSERT INTO clients(id,curator_id,name,invite_code,created_at)
                 VALUES (?,?,?,?,?)",
                vec![
                    id.as_str().into(),
                    curator_id.into(),
                    name.into(),
                    code.as_str().into(),
                    now.as_str().into(),
                ],
            ) {
                Ok(_) => {
                    let row = self.client(curator_id, &id)?.ok_or_else(|| {
                        Error::RustError("client vanished right after insert".into())
                    })?;
                    return Response::from_json(&serde_json::json!({
                        "client": ClientRow::from(row),
                    }));
                }
                Err(_) => code = generate_invite_code()?,
            }
        }
        Err(Error::RustError(
            "could not allocate a unique invite code in 8 attempts".into(),
        ))
    }

    fn client(&self, curator_id: &str, id: &str) -> Result<Option<ClientSqlRow>> {
        let sql = self.state.storage().sql();
        let rows: Vec<ClientSqlRow> = sql
            .exec(
                "SELECT id, name, invite_code, user_id, bound_at, unbound_at,
                        last_report_at, request_days, request_at
                 FROM clients WHERE curator_id = ? AND id = ?",
                vec![curator_id.into(), id.into()],
            )?
            .to_array()?;
        Ok(rows.into_iter().next())
    }

    fn handle_client_list(&self, curator_id: &str) -> Result<Response> {
        let sql = self.state.storage().sql();
        let rows: Vec<ClientSqlRow> = sql
            .exec(
                "SELECT id, name, invite_code, user_id, bound_at, unbound_at,
                        last_report_at, request_days, request_at
                 FROM clients WHERE curator_id = ? ORDER BY created_at",
                vec![curator_id.into()],
            )?
            .to_array()?;
        let clients: Vec<ClientRow> = rows.into_iter().map(ClientRow::from).collect();
        Response::from_json(&serde_json::json!({ "clients": clients }))
    }

    fn handle_client_get(&self, curator_id: &str, id: &str) -> Result<Response> {
        match self.client(curator_id, id)? {
            Some(row) => Response::from_json(&serde_json::json!({
                "found": true, "client": ClientRow::from(row),
            })),
            None => Response::from_json(&serde_json::json!({ "found": false })),
        }
    }

    fn handle_client_rename(&self, curator_id: &str, id: &str, name: &str) -> Result<Response> {
        if self.client(curator_id, id)?.is_none() {
            return Response::error("client not found", 404);
        }
        let sql = self.state.storage().sql();
        sql.exec(
            "UPDATE clients SET name = ? WHERE curator_id = ? AND id = ?",
            vec![name.into(), curator_id.into(), id.into()],
        )?;
        let row = self
            .client(curator_id, id)?
            .ok_or_else(|| Error::RustError("client vanished mid-rename".into()))?;
        Response::from_json(&serde_json::json!({ "client": ClientRow::from(row) }))
    }

    /// Слот по пригласительному коду. Код ищется по ВСЕМ кураторам — он и есть
    /// адрес приглашения.
    fn by_invite(&self, code: &str) -> Result<Option<OwnedClientRow>> {
        let sql = self.state.storage().sql();
        let rows: Vec<OwnedClientRow> = sql
            .exec(
                "SELECT id, curator_id, user_id FROM clients WHERE invite_code = ?",
                vec![code.into()],
            )?
            .to_array()?;
        Ok(rows.into_iter().next())
    }

    /// Привязанный слот этого худеющего, если он есть. Единственный по построению
    /// (частичный UNIQUE по user_id).
    fn bound_of_user(&self, user_id: &str) -> Result<Option<OwnedClientRow>> {
        let sql = self.state.storage().sql();
        let rows: Vec<OwnedClientRow> = sql
            .exec(
                "SELECT id, curator_id, user_id FROM clients WHERE user_id = ?",
                vec![user_id.into()],
            )?
            .to_array()?;
        Ok(rows.into_iter().next())
    }

    /// Что показать на экране согласия: имя куратора и не он ли это уже.
    fn handle_invite_peek(&self, code: &str, user_id: &str) -> Result<Response> {
        let Some(row) = self.by_invite(code)? else {
            return Response::from_json(&serde_json::json!({ "found": false }));
        };
        // Код гасится согласием, а не открытием: пока слот не привязан, ссылку
        // можно переслать ещё раз. Привязанный слот кода уже не отдаёт.
        if row.user_id.is_some() {
            return Response::from_json(&serde_json::json!({ "found": false, "used": true }));
        }
        let curator = self.curator(&row.curator_id)?;
        let current = self.bound_of_user(user_id)?;
        Response::from_json(&serde_json::json!({
            "found": true,
            "curator_name": curator.and_then(|c| c.name).unwrap_or_default(),
            "client_id": row.id,
            // Уже у кого-то: экран согласия обязан сказать, что старая связь
            // оборвётся, а не молча её оборвать.
            "current_curator_id": current.as_ref().map(|c| c.curator_id.clone()),
        }))
    }

    /// Снять привязку со слота: user_id в NULL, отметка времени и НОВЫЙ код —
    /// прежний погашен согласием и второй раз работать не должен. Слот остаётся
    /// в списке куратора: тем же слотом человека приглашают снова.
    fn unbind_row(&self, id: &str) -> Result<()> {
        let sql = self.state.storage().sql();
        let now = now_rfc3339();
        let mut code = generate_invite_code()?;
        for _ in 0..8 {
            match sql.exec(
                "UPDATE clients
                 SET user_id = NULL, unbound_at = ?, invite_code = ?,
                     request_days = NULL, request_at = NULL,
                     last_report_at = NULL, last_report = NULL
                 WHERE id = ?",
                vec![now.as_str().into(), code.as_str().into(), id.into()],
            ) {
                Ok(_) => return Ok(()),
                Err(_) => code = generate_invite_code()?,
            }
        }
        Err(Error::RustError(
            "could not allocate a unique invite code on unbind in 8 attempts".into(),
        ))
    }

    /// Согласие худеющего. Если он уже у другого куратора — прежняя связь рвётся
    /// здесь же, одной операцией: два куратора у одного человека не бывает, и
    /// оставлять решение вызывающей стороне значит допустить окно, в котором их
    /// двое. Прежний слот возвращается в список прежнего куратора с новым кодом.
    fn handle_invite_accept(&self, code: &str, user_id: &str) -> Result<Response> {
        let Some(row) = self.by_invite(code)? else {
            return Response::error("invite not found", 404);
        };
        if row.user_id.is_some() {
            return Response::error("invite already used", 409);
        }
        let previous = self.bound_of_user(user_id)?;
        if let Some(prev) = &previous {
            if prev.id == row.id {
                return Response::error("already bound to this client", 409);
            }
            self.unbind_row(&prev.id)?;
        }
        let sql = self.state.storage().sql();
        let now = now_rfc3339();
        sql.exec(
            "UPDATE clients SET user_id = ?, bound_at = ?, unbound_at = NULL WHERE id = ?",
            vec![user_id.into(), now.as_str().into(), row.id.as_str().into()],
        )?;
        let curator = self.curator(&row.curator_id)?;
        Response::from_json(&serde_json::json!({
            "ok": true,
            "client_id": row.id,
            "curator_id": row.curator_id,
            "curator_name": curator.and_then(|c| c.name).unwrap_or_default(),
            "previous": previous.map(|p| serde_json::json!({
                "curator_id": p.curator_id, "client_id": p.id,
            })),
        }))
    }

    /// Отвязка. Инициатор — либо куратор (тогда задан `curator_id`, и чужой слот
    /// не найдётся), либо сам худеющий (тогда задан `user_id`).
    fn handle_unbind(
        &self,
        curator_id: Option<&str>,
        id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Response> {
        let row = match (curator_id, id, user_id) {
            (Some(cid), Some(id), _) => match self.client(cid, id)? {
                Some(r) => OwnedClientRow {
                    id: r.id,
                    curator_id: cid.to_string(),
                    user_id: r.user_id,
                },
                None => return Response::error("client not found", 404),
            },
            (_, _, Some(uid)) => match self.bound_of_user(uid)? {
                Some(r) => r,
                None => return Response::error("not bound", 404),
            },
            _ => return Response::error("need client or user", 400),
        };
        let Some(bound_user) = row.user_id.clone() else {
            return Response::error("not bound", 404);
        };
        self.unbind_row(&row.id)?;
        Response::from_json(&serde_json::json!({
            "ok": true,
            "client_id": row.id,
            "curator_id": row.curator_id,
            "user_id": bound_user,
        }))
    }

    /// Кто курирует этого худеющего. Это же — маршрут его сообщений.
    fn handle_binding(&self, user_id: &str) -> Result<Response> {
        let Some(row) = self.bound_of_user(user_id)? else {
            return Response::from_json(&serde_json::json!({ "bound": false }));
        };
        let curator = self.curator(&row.curator_id)?;
        Response::from_json(&serde_json::json!({
            "bound": true,
            "curator_id": row.curator_id,
            "client_id": row.id,
            "curator_name": curator.and_then(|c| c.name).unwrap_or_default(),
        }))
    }

    /// Удаление слота. Возвращает `user_id`, если слот был привязан, — вызывающая
    /// сторона обязана довести отвязку до конца (письмо худеющему, чистка треда).
    fn handle_client_delete(&self, curator_id: &str, id: &str) -> Result<Response> {
        let Some(row) = self.client(curator_id, id)? else {
            return Response::error("client not found", 404);
        };
        let sql = self.state.storage().sql();
        sql.exec(
            "DELETE FROM clients WHERE curator_id = ? AND id = ?",
            vec![curator_id.into(), id.into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true, "user_id": row.user_id }))
    }
}

/// Тот же алфавит без похожих символов, что у кода одобрения эксперта: код может
/// быть продиктован голосом. 10 символов над 32 = 50 бит — это не одобрение
/// оператором, а публичная ссылка, и подбор ей вреден.
const INVITE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const INVITE_LEN: usize = 10;

/// CSPRNG (в wasm — crypto.getRandomValues). 256 % 32 == 0, поэтому остаток
/// строго равномерен и смещения нет.
fn generate_invite_code() -> Result<String> {
    let mut buf = [0u8; INVITE_LEN];
    getrandom::getrandom(&mut buf).map_err(|e| Error::RustError(format!("getrandom: {e}")))?;
    Ok(buf
        .iter()
        .map(|b| INVITE_ALPHABET[(*b as usize) % INVITE_ALPHABET.len()] as char)
        .collect())
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn str_field<'a>(body: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    body.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError(format!("missing {key}")))
}

impl DurableObject for CuratorIndexDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        let path = req.url()?.path().to_string();
        let body: serde_json::Value = req.json().await.unwrap_or(serde_json::Value::Null);

        match path.as_str() {
            "/curator-register" => self.handle_register(str_field(&body, "curator_id")?),
            "/curator-get" => self.handle_get(str_field(&body, "curator_id")?),
            "/curator-set" => self.handle_set(
                str_field(&body, "curator_id")?,
                body.get("name").and_then(|v| v.as_str()),
                body.get("lang").and_then(|v| v.as_str()),
            ),
            "/client-create" => self.handle_client_create(
                str_field(&body, "curator_id")?,
                str_field(&body, "name")?,
            ),
            "/client-list" => self.handle_client_list(str_field(&body, "curator_id")?),
            "/client-get" => {
                self.handle_client_get(str_field(&body, "curator_id")?, str_field(&body, "id")?)
            }
            "/client-rename" => self.handle_client_rename(
                str_field(&body, "curator_id")?,
                str_field(&body, "id")?,
                str_field(&body, "name")?,
            ),
            "/client-delete" => {
                self.handle_client_delete(str_field(&body, "curator_id")?, str_field(&body, "id")?)
            }
            "/invite-peek" => self.handle_invite_peek(
                str_field(&body, "code")?,
                str_field(&body, "user_id")?,
            ),
            "/invite-accept" => self.handle_invite_accept(
                str_field(&body, "code")?,
                str_field(&body, "user_id")?,
            ),
            "/unbind" => self.handle_unbind(
                body.get("curator_id").and_then(|v| v.as_str()),
                body.get("id").and_then(|v| v.as_str()),
                body.get("user_id").and_then(|v| v.as_str()),
            ),
            "/binding" => self.handle_binding(str_field(&body, "user_id")?),
            _ => Response::error(format!("unknown DO path: {path}"), 404),
        }
    }
}
