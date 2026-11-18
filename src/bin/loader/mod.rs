//! SD-card game discovery.
//!
//! The convention is one directory per game in the FAT volume root:
//!
//! ```text
//! /snake/manifest.json
//! /snake/snake.wasm
//! /chess/manifest.json
//! /chess/chess.wasm
//! ```
//!
//! `manifest.json` is a small JSON object `{"id", "title", "wasm"}`; `wasm` is
//! the path (relative to the game directory) of the compiled game module.

use alloc::vec::Vec;
use core::ops::ControlFlow;

use embedded_sdmmc::{BlockDevice, Directory, Mode, ShortFileName, TimeSource};
use heapless::String;
use heapless::Vec as HVec;
use serde::Deserialize;

use crate::game::{GameMeta, MAX_GAMES};

/// Upper bound for a single compiled game module. Games must fit comfortably in
/// the reclaimed heap; 64 KiB is a generous starting budget.
pub const MAX_WASM_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct GameManifest {
    id: String<32>,
    title: String<32>,
    wasm: String<64>,
}

pub struct DiscoveredGame {
    pub meta: GameMeta,
    pub wasm: Vec<u8>,
}

#[derive(Debug)]
pub enum LoadError {
    Sd,
    BadManifest,
    Json,
    WasmTooLarge,
    TooManyGames,
}

pub fn discover<D, T, const MD: usize, const MF: usize, const MV: usize>(
    root: &Directory<'_, D, T, MD, MF, MV>,
) -> Result<Vec<DiscoveredGame>, LoadError>
where
    D: BlockDevice,
    T: TimeSource,
{
    let mut games = Vec::<DiscoveredGame>::new();

    // Collect directory names first; we cannot open a sub-directory inside the
    // `iterate_dir` callback without fighting the borrow checker.
    let mut dirs = HVec::<ShortFileName, 16>::new();
    let _ = root.iterate_dir(|entry| {
        if entry.attributes.is_directory()
            && entry.name != ShortFileName::this_dir()
            && entry.name != ShortFileName::parent_dir()
        {
            let _ = dirs.push(entry.name);
        }
        ControlFlow::Continue(())
    });

    for name in dirs.iter() {
        let dir = root.open_dir(*name).map_err(|_| LoadError::Sd)?;

        let Ok(manifest_bytes) = read_file_to_vec(&dir, "manifest.json", 512) else {
            continue;
        };
        let Ok(manifest) = parse_manifest(&manifest_bytes) else {
            continue;
        };
        let Ok(wasm) = read_file_to_vec(&dir, manifest.wasm.as_str(), MAX_WASM_BYTES) else {
            continue;
        };

        if games.len() >= MAX_GAMES {
            return Err(LoadError::TooManyGames);
        }

        games.push(DiscoveredGame {
            meta: GameMeta {
                id: manifest.id,
                title: manifest.title,
            },
            wasm,
        });
    }

    Ok(games)
}

fn parse_manifest(bytes: &[u8]) -> Result<GameManifest, LoadError> {
    let text = core::str::from_utf8(bytes).map_err(|_| LoadError::BadManifest)?;
    let (manifest, _) =
        serde_json_core::from_str::<GameManifest>(text).map_err(|_| LoadError::Json)?;
    Ok(manifest)
}

fn read_file_to_vec<D, T, const MD: usize, const MF: usize, const MV: usize>(
    dir: &Directory<'_, D, T, MD, MF, MV>,
    name: &str,
    max: usize,
) -> Result<Vec<u8>, LoadError>
where
    D: BlockDevice,
    T: TimeSource,
{
    let file = dir
        .open_file_in_dir(name, Mode::ReadOnly)
        .map_err(|_| LoadError::Sd)?;

    let mut data = Vec::<u8>::new();
    let mut buffer = [0u8; 256];
    while !file.is_eof() {
        let n = file.read(&mut buffer).map_err(|_| LoadError::Sd)?;
        if n == 0 {
            break;
        }
        if data.len().saturating_add(n) > max {
            return Err(LoadError::WasmTooLarge);
        }
        data.extend_from_slice(&buffer[..n]);
    }
    Ok(data)
}
