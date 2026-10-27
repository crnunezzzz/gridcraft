//! The AI assistant panel shared by GridCraft's hosts: the panel and the provider protocols
//! compile everywhere; [`desktop`] runs them on Windows, macOS and Linux (the Android host is
//! `gridcraft-android`).
#![deny(unsafe_code, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]

pub mod agent;
pub mod assistant;
pub mod attach;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub mod desktop;
pub mod protocol;
pub mod update;

/// Where this fork publishes its builds (the workflow's own repository takes precedence).
pub const RELEASES: &str = "crnunezzzz/gridcraft";

/// The assistant's texts for this app.
pub const BRANDING: assistant::Branding = assistant::Branding {
    name: "GridCraft",
    noun: "el libro",
    welcome: "Pregunta sobre la hoja de cálculo o pide cambios. El asistente la lee y la edita con las mismas órdenes que la línea de comandos.",
    suggestions: &[
        ("Explica los datos", "Explica qué datos contiene el libro y qué muestran."),
        ("Añade totales", "Añade una fila de totales con fórmulas SUMA debajo de los datos de la hoja activa."),
        ("Crea un gráfico", "Crea un gráfico adecuado para los datos de la hoja activa."),
        ("Da formato de tabla", "Aplica formato de tabla a los datos de la hoja activa, con los encabezados en negrita."),
    ],
};
