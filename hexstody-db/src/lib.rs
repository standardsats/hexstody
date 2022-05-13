pub mod queries;
pub mod state;
pub mod update;

use log::*;
use queries::insert_update;
use sqlx::postgres::{PgPoolOptions, Postgres};
use state::*;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::{Mutex, Notify};
use update::*;

pub type Pool = sqlx::Pool<Postgres>;

pub async fn create_db_pool(conn_string: &str) -> Result<Pool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(conn_string)
        .await?;

    sqlx::migrate!("../hexstody-db/migrations")
        .run(&pool)
        .await?;

    Ok(pool)
}

pub async fn update_worker(
    pool: Pool,
    state: Arc<Mutex<State>>,
    state_notify: Arc<Notify>,
    mut update_receiver: Receiver<StateUpdate>,
) {
    info!("Update state worker started");
    while let Some(i) = update_receiver.recv().await {
        debug!("Applying state update: {:?}", i);
        {
            let mut mstate = state.lock().await;
            let mut copy_state = mstate.clone();
            match copy_state.apply_update(i.clone()) {
                Ok(_) => match insert_update(&pool, i.body, Some(i.created)).await {
                    Ok(_) => {
                        *mstate = copy_state;
                    }
                    Err(e) => {
                        error!("Failed to store state update, reverting: {:?}", e);
                        continue;
                    }
                },
                Err(e) => {
                    error!("Failed to apply state update: {:?}", e);
                    continue;
                }
            }
        }
        state_notify.notify_waiters();
    }
    info!("Update state worker exited!");
}
