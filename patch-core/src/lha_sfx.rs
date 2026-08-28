use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use anyhow::{Context, Result, ensure};
use delharc::LhaDecodeReader;

use crate::limits::MAX_LHA_ENTRIES;

pub(crate) fn extract_mz_lha_members(
    executable: &[u8],
    expected_members: &BTreeMap<String, usize>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let archive_offset = mz_executable_length(executable)?;
    let archive = executable
        .get(archive_offset..)
        .context("MZ executable does not contain an appended LHA archive")?;
    ensure!(
        archive
            .get(2..7)
            .is_some_and(|method| method.starts_with(b"-lh")),
        "LHA archive does not start at the MZ-declared executable boundary"
    );
    let mut reader = LhaDecodeReader::new(Cursor::new(archive))
        .map_err(|error| anyhow::anyhow!("parse appended LHA header: {error}"))?;
    let mut files = BTreeMap::new();
    let mut entry_count = 0usize;
    loop {
        entry_count = entry_count
            .checked_add(1)
            .context("LHA entry count overflow")?;
        ensure!(
            entry_count <= MAX_LHA_ENTRIES,
            "LHA archive has more than {MAX_LHA_ENTRIES} entries"
        );
        let name = reader.header().parse_pathname_to_str().to_ascii_uppercase();
        if let Some(&expected_size) = expected_members.get(&name) {
            ensure!(
                !files.contains_key(&name),
                "duplicate required LHA member name: {name}"
            );
            ensure!(
                reader.is_decoder_supported(),
                "unsupported LHA method for {name}: {:?}",
                reader.header().compression_method()
            );
            let announced_size = usize::try_from(reader.header().original_size)
                .with_context(|| format!("LHA member {name} size does not fit memory"))?;
            ensure!(
                announced_size == expected_size,
                "LHA member {name} size mismatch: expected {expected_size}, header declares {announced_size}"
            );
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(expected_size)
                .with_context(|| format!("reserve LHA member {name} buffer"))?;
            let read_limit = u64::try_from(expected_size)
                .context("LHA member size does not fit decoder limits")?
                .checked_add(1)
                .context("LHA member decode limit overflow")?;
            (&mut reader)
                .take(read_limit)
                .read_to_end(&mut bytes)
                .with_context(|| format!("decode LHA member {name}"))?;
            ensure!(
                bytes.len() == expected_size,
                "LHA member {name} decoded to {} bytes, expected {expected_size}",
                bytes.len()
            );
            reader
                .crc_check()
                .map_err(|error| anyhow::anyhow!("LHA CRC check failed for {name}: {error}"))?;
            files.insert(name, bytes);
            if files.len() == expected_members.len() {
                return Ok(files);
            }
        }
        if !reader
            .seek_next_file()
            .map_err(|error| anyhow::anyhow!("parse next LHA member: {error}"))?
        {
            break;
        }
    }
    for name in expected_members.keys() {
        ensure!(
            files.contains_key(name),
            "LHA archive is missing member {name}"
        );
    }
    Ok(files)
}

fn mz_executable_length(executable: &[u8]) -> Result<usize> {
    ensure!(
        executable.len() >= 6,
        "MZ executable has a truncated header"
    );
    ensure!(
        &executable[..2] == b"MZ",
        "LHA container is not an MZ executable"
    );
    let last_page_bytes = usize::from(u16::from_le_bytes([executable[2], executable[3]]));
    let page_count = usize::from(u16::from_le_bytes([executable[4], executable[5]]));
    ensure!(page_count > 0, "MZ header declares zero pages");
    ensure!(
        last_page_bytes <= 512,
        "MZ header has an invalid last-page size"
    );
    let length = if last_page_bytes == 0 {
        page_count.checked_mul(512).context("MZ length overflow")?
    } else {
        page_count
            .checked_sub(1)
            .and_then(|pages| pages.checked_mul(512))
            .and_then(|bytes| bytes.checked_add(last_page_bytes))
            .context("MZ length overflow")?
    };
    ensure!(
        length < executable.len(),
        "MZ header does not leave an appended archive"
    );
    Ok(length)
}

#[cfg(test)]
#[path = "lha_sfx_tests.rs"]
mod lha_sfx_tests;
