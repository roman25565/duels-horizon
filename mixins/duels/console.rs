// A command typed on the dedicated server's console: ours sets `ran`.
fn duels_console(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if !*ran && crate::game::duels_glue::console(game, command) {
        *ran = true;
    }
}
