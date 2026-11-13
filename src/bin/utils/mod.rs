pub mod config;
pub mod render_menu;

use embassy_net::Stack;

/// Coarse connection status for the menu. Game-specific status (e.g. chess
/// server state) stays inside the game module.
pub fn wifi_status(stack: &Stack<'static>) -> &'static str {
    if stack.is_link_up() {
        if stack.is_config_up() {
            "Wi-Fi connected"
        } else {
            "DHCP..."
        }
    } else {
        "Wi-Fi..."
    }
}
