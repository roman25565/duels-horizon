// The world closing: the duels' state is written and let go.
fn duels_world_lifecycle(phase: horizon_client::mod_api::WorldPhase) {
    if matches!(
        phase,
        horizon_client::mod_api::WorldPhase::WorldCloseFinal
            | horizon_client::mod_api::WorldPhase::WorldCloseFailed
    ) {
        crate::game::duels_glue::closed();
    }
}
