/// Look up a built-in theme by name, returning its TOML source.
///
/// Theme names correspond to files in `examples/themes/`.
/// Pass a bare name (e.g. `"dracula"`), not the full filename.
pub fn get(name: &str) -> Option<&'static str> {
    match name {
        "ayu-dark" => Some(include_str!("../../examples/themes/ayu-dark.toml")),
        "base16-default" => Some(include_str!("../../examples/themes/base16-default.toml")),
        "catppuccin-latte" => Some(include_str!("../../examples/themes/catppuccin-latte.toml")),
        "catppuccin-mocha" => Some(include_str!("../../examples/themes/catppuccin-mocha.toml")),
        "dracula" => Some(include_str!("../../examples/themes/dracula.toml")),
        "everforest" => Some(include_str!("../../examples/themes/everforest.toml")),
        "gruvbox-dark" => Some(include_str!("../../examples/themes/gruvbox-dark.toml")),
        "iceberg" => Some(include_str!("../../examples/themes/iceberg.toml")),
        "kanagawa" => Some(include_str!("../../examples/themes/kanagawa.toml")),
        "modus-operandi" => Some(include_str!("../../examples/themes/modus-operandi.toml")),
        "modus-vivendi" => Some(include_str!("../../examples/themes/modus-vivendi.toml")),
        "monokai" => Some(include_str!("../../examples/themes/monokai.toml")),
        "nightfox" => Some(include_str!("../../examples/themes/nightfox.toml")),
        "night-owl" => Some(include_str!("../../examples/themes/night-owl.toml")),
        "nord" => Some(include_str!("../../examples/themes/nord.toml")),
        "one-dark" => Some(include_str!("../../examples/themes/one-dark.toml")),
        "onehalf-dark" => Some(include_str!("../../examples/themes/onehalf-dark.toml")),
        "palenight" => Some(include_str!("../../examples/themes/palenight.toml")),
        "rose-pine" => Some(include_str!("../../examples/themes/rose-pine.toml")),
        "solarized-16" => Some(include_str!("../../examples/themes/solarized-16.toml")),
        "solarized-dark" => Some(include_str!("../../examples/themes/solarized-dark.toml")),
        "solarized-light" => Some(include_str!("../../examples/themes/solarized-light.toml")),
        "srcery" => Some(include_str!("../../examples/themes/srcery.toml")),
        "tokyo-night" => Some(include_str!("../../examples/themes/tokyo-night.toml")),
        "zenburn" => Some(include_str!("../../examples/themes/zenburn.toml")),
        _ => None,
    }
}

/// List all built-in theme names in alphabetical order.
pub fn list() -> &'static [&'static str] {
    &[
        "ayu-dark",
        "base16-default",
        "catppuccin-latte",
        "catppuccin-mocha",
        "dracula",
        "everforest",
        "gruvbox-dark",
        "iceberg",
        "kanagawa",
        "modus-operandi",
        "modus-vivendi",
        "monokai",
        "nightfox",
        "night-owl",
        "nord",
        "one-dark",
        "onehalf-dark",
        "palenight",
        "rose-pine",
        "solarized-16",
        "solarized-dark",
        "solarized-light",
        "srcery",
        "tokyo-night",
        "zenburn",
    ]
}
