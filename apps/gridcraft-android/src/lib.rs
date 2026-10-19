//! Android host. The document engine and egui editor remain the desktop implementations.
#![deny(unsafe_code, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]

#[cfg(target_os = "android")]
mod adapter;
pub mod assistant;
pub mod attach;
pub mod input;
#[cfg(target_os = "android")]
mod mobile;
#[cfg(target_os = "android")]
mod platform;
pub mod protocol;

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

// android-activity requires this exact exported Rust ABI symbol. No unsafe operations occur
// here or in the platform bridge; JNI uses checked calls and JavaVM::singleton().
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub fn android_main(app: android_activity::AndroidApp) {
    mobile::start(app);
}
