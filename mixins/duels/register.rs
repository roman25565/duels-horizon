// The duel commands in every player's command tree.
fn duels_register(commands: &mut Vec<horizon_client::mod_api::NativeCommand>) {
    crate::game::duels_glue::register(commands);
}
