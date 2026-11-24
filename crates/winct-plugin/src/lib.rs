//! Runtime plugin WebAssembly untuk Winct.
//!
//! Guest berjalan di Wasmtime dengan fuel terbatas dan hanya bisa memanggil host lewat satu fungsi
//! `winc.host_call`. Host memeriksa capability sebelum meneruskan request ke [`HostBridge`]. Request
//! yang mengubah state tidak langsung menyentuh `App`, melainkan masuk antrean [`HostEffect`] yang
//! diterapkan oleh `winct-app` setelah panggilan guest selesai. Ini menghindari reentrancy dan
//! meminjam `App` dua kali.

// `HostEffect` adalah wire format internal antar-thread; nama variant sudah deskriptif.
#![allow(missing_docs)]

mod runtime;

/// ABI tipe yang dipakai host dan guest; di-re-export supaya `winct-app` cukup bergantung pada
/// crate runtime saja.
pub use winct_plugin_api as api;

pub use runtime::{
    FUEL, HostBridge, HostEffect, HostState, PluginEffect, PluginError, PluginInstance,
    PluginRuntime,
};

/// Batas ukuran pesan yang dipakai ulang dari ABI.
pub use winct_plugin_api::MESSAGE_LIMIT;
