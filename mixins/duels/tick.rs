// The end of each server tick: challenges, duels and the files.
fn duels_server_tick(game: &mut crate::game::Game<'_>, phase: horizon_client::mod_api::ServerTickPhase) {
    if matches!(
        phase,
        horizon_client::mod_api::ServerTickPhase::ServerTickEnd { .. }
    ) {
        crate::game::duels_glue::tick(game);
    }
}
