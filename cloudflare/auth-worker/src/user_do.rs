use worker::*;

// UserDO is kept as an empty shell for future sync data purposes.
// All authentication logic has moved to AuthDO.

#[durable_object]
pub struct UserDO {
    state: worker::durable::State,
    env: Env,
}

impl DurableObject for UserDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, _req: Request) -> Result<Response> {
        Response::error("UserDO is deprecated — use AuthDO", 404)
    }
}
