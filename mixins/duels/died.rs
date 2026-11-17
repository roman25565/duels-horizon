// A server player died: a duel may be over (handled at the next tick).
fn duels_player_died(player: u128) {
    crate::game::duels_glue::died(player);
}
