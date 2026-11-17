// A player's command: ours sets `ran`.
fn duels_perform(
    game: &mut crate::game::Game<'_>,
    player: &horizon_client::mod_api::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    if !*ran && crate::game::duels_glue::perform(game, player, command) {
        *ran = true;
    }
}
