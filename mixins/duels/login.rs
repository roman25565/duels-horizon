// A player joined: their missed lines, and the way back from a duel they left.
fn duels_logged_in(player: u128, name: &str) {
    crate::game::duels_glue::logged_in(player, name);
}
