use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};

pub(crate) const LOGICAL_SECTOR_SIZE: usize = 2_048;
pub(crate) const RAW_MODE1_SECTOR_SIZE: usize = 2_352;
const RAW_MODE1_DATA_OFFSET: usize = 16;
const PRIMARY_VOLUME_DESCRIPTOR_LBA: usize = 16;
const VOLUME_DESCRIPTOR_TERMINATOR_LBA: usize = 17;
const ROOT_DIRECTORY_LBA: usize = 20;
const FIXED_RECORDING_DATE: [u8; 7] = [94, 1, 1, 0, 0, 0, 0];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IsoFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct DirectoryRecord {
    name: String,
    extent_lba: usize,
    data_length: usize,
    is_directory: bool,
}

pub(crate) fn extract_raw_mode1_directory(
    image: &[u8],
    expected_volume_id: &str,
    directory_path: &str,
) -> Result<Vec<IsoFile>> {
    ensure!(
        image.len().is_multiple_of(RAW_MODE1_SECTOR_SIZE),
        "source CD image size is not a whole number of 2352-byte raw sectors"
    );
    let pvd = raw_user_sector(image, PRIMARY_VOLUME_DESCRIPTOR_LBA)?;
    let root = verify_pvd(pvd, expected_volume_id)?;
    let directory = resolve_directory(|lba| raw_user_sector(image, lba), &root, directory_path)?;
    extract_directory_files(|lba| raw_user_sector(image, lba), &directory)
}

pub(crate) fn extract_logical_directory(
    image: &[u8],
    expected_volume_id: &str,
    directory_path: &str,
) -> Result<Vec<IsoFile>> {
    ensure!(
        image.len().is_multiple_of(LOGICAL_SECTOR_SIZE),
        "ISO image size is not a whole number of 2048-byte logical sectors"
    );
    let pvd = logical_sector(image, PRIMARY_VOLUME_DESCRIPTOR_LBA)?;
    let root = verify_pvd(pvd, expected_volume_id)?;
    let volume_sectors = read_both_endian_u32(pvd, 80, "volume space size")? as usize;
    ensure!(
        volume_sectors == image.len() / LOGICAL_SECTOR_SIZE,
        "ISO volume space size differs from image length"
    );
    let directory = resolve_directory(|lba| logical_sector(image, lba), &root, directory_path)?;
    extract_directory_files(|lba| logical_sector(image, lba), &directory)
}

pub(crate) fn build_single_directory_iso(
    volume_id: &str,
    directory_name: &str,
    files: &[IsoFile],
) -> Result<Vec<u8>> {
    validate_volume_id(volume_id)?;
    validate_directory_identifier(directory_name)?;
    ensure!(!files.is_empty(), "ISO output directory cannot be empty");

    let mut files = files.to_vec();
    files.sort_by(|left, right| left.name.cmp(&right.name));

    let mut names = BTreeSet::new();
    for file in &files {
        validate_file_identifier(&file.name)?;
        ensure!(
            names.insert(file.name.as_str()),
            "duplicate ISO output filename {}",
            file.name
        );
    }

    let root_records = vec![
        directory_record_bytes(u32::try_from(ROOT_DIRECTORY_LBA).unwrap(), 0, true, &[0])?,
        directory_record_bytes(u32::try_from(ROOT_DIRECTORY_LBA).unwrap(), 0, true, &[1])?,
        directory_record_bytes(0, 0, true, directory_name.as_bytes())?,
    ];
    let root_directory_size = packed_directory_size(&root_records)?;
    let root_directory_sectors = root_directory_size.div_ceil(LOGICAL_SECTOR_SIZE);
    ensure!(
        root_directory_sectors == 1,
        "single-directory ISO root unexpectedly exceeds one sector"
    );

    let placeholder_file_records = files
        .iter()
        .map(|file| {
            directory_record_bytes(
                0,
                file.bytes.len(),
                false,
                format!("{};1", file.name).as_bytes(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let mut directory_records = vec![
        directory_record_bytes(0, 0, true, &[0])?,
        directory_record_bytes(
            u32::try_from(ROOT_DIRECTORY_LBA).unwrap(),
            root_directory_size,
            true,
            &[1],
        )?,
    ];
    directory_records.extend(placeholder_file_records);
    let output_directory_size = packed_directory_size(&directory_records)?;
    let output_directory_sectors = output_directory_size.div_ceil(LOGICAL_SECTOR_SIZE);
    let output_directory_lba = ROOT_DIRECTORY_LBA + root_directory_sectors;
    let path_table_l_lba = 18usize;
    let path_table_m_lba = 19usize;
    ensure!(
        output_directory_lba > path_table_m_lba,
        "ISO metadata layout overlaps directories"
    );

    let first_file_lba = output_directory_lba + output_directory_sectors;
    let mut next_file_lba = first_file_lba;
    let mut file_extents = BTreeMap::new();
    for file in &files {
        file_extents.insert(file.name.clone(), next_file_lba);
        next_file_lba = next_file_lba
            .checked_add(file.bytes.len().div_ceil(LOGICAL_SECTOR_SIZE))
            .context("ISO file extent allocation overflow")?;
    }
    let volume_sectors = next_file_lba;
    ensure!(
        volume_sectors <= u32::MAX as usize,
        "ISO output exceeds 32-bit volume space"
    );

    let path_table_l = build_path_table(
        false,
        directory_name,
        ROOT_DIRECTORY_LBA,
        output_directory_lba,
    )?;
    let path_table_m = build_path_table(
        true,
        directory_name,
        ROOT_DIRECTORY_LBA,
        output_directory_lba,
    )?;
    ensure!(
        path_table_l.len() <= LOGICAL_SECTOR_SIZE && path_table_m.len() <= LOGICAL_SECTOR_SIZE,
        "ISO path table exceeds its reserved sector"
    );

    let mut output = vec![0u8; volume_sectors * LOGICAL_SECTOR_SIZE];
    let root_directory_size_u32 =
        u32::try_from(root_directory_size).context("ISO root directory size exceeds 32 bits")?;
    let output_directory_lba_u32 =
        u32::try_from(output_directory_lba).context("ISO output directory LBA exceeds 32 bits")?;

    let root_records = vec![
        directory_record_bytes(
            u32::try_from(ROOT_DIRECTORY_LBA).unwrap(),
            root_directory_size_u32 as usize,
            true,
            &[0],
        )?,
        directory_record_bytes(
            u32::try_from(ROOT_DIRECTORY_LBA).unwrap(),
            root_directory_size_u32 as usize,
            true,
            &[1],
        )?,
        directory_record_bytes(
            output_directory_lba_u32,
            output_directory_size,
            true,
            directory_name.as_bytes(),
        )?,
    ];
    let mut output_records = vec![
        directory_record_bytes(output_directory_lba_u32, output_directory_size, true, &[0])?,
        directory_record_bytes(
            u32::try_from(ROOT_DIRECTORY_LBA).unwrap(),
            root_directory_size,
            true,
            &[1],
        )?,
    ];
    for file in &files {
        output_records.push(directory_record_bytes(
            u32::try_from(file_extents[&file.name]).context("ISO file LBA exceeds 32 bits")?,
            file.bytes.len(),
            false,
            format!("{};1", file.name).as_bytes(),
        )?);
    }

    write_pvd(
        sector_mut(&mut output, PRIMARY_VOLUME_DESCRIPTOR_LBA)?,
        volume_id,
        u32::try_from(volume_sectors).unwrap(),
        u32::try_from(path_table_l.len()).context("ISO path table size exceeds 32 bits")?,
        u32::try_from(path_table_l_lba).unwrap(),
        u32::try_from(path_table_m_lba).unwrap(),
        &root_records[0],
    )?;
    write_volume_terminator(sector_mut(&mut output, VOLUME_DESCRIPTOR_TERMINATOR_LBA)?);
    sector_mut(&mut output, path_table_l_lba)?[..path_table_l.len()].copy_from_slice(&path_table_l);
    sector_mut(&mut output, path_table_m_lba)?[..path_table_m.len()].copy_from_slice(&path_table_m);
    write_packed_directory(&mut output, ROOT_DIRECTORY_LBA, &root_records)?;
    write_packed_directory(&mut output, output_directory_lba, &output_records)?;

    for file in &files {
        let start = file_extents[&file.name] * LOGICAL_SECTOR_SIZE;
        output[start..start + file.bytes.len()].copy_from_slice(&file.bytes);
    }

    let extracted = extract_logical_directory(&output, volume_id, directory_name)
        .context("reparse newly built ISO")?;
    ensure!(
        extracted == files,
        "new ISO did not reproduce its declared directory files"
    );
    Ok(output)
}

fn verify_pvd(pvd: &[u8], expected_volume_id: &str) -> Result<DirectoryRecord> {
    ensure!(
        pvd[0] == 1 && &pvd[1..6] == b"CD001" && pvd[6] == 1,
        "source CD has no ISO 9660 primary volume descriptor at LBA 16"
    );
    let volume_id = String::from_utf8_lossy(&pvd[40..72]).trim().to_owned();
    ensure!(
        volume_id == expected_volume_id,
        "unexpected ISO volume identifier: expected {expected_volume_id}, got {volume_id:?}"
    );
    parse_directory_record(&pvd[156..])?
        .context("ISO primary volume descriptor has no root directory record")
}

fn resolve_directory<'a, F>(
    mut read_sector: F,
    root: &DirectoryRecord,
    path: &str,
) -> Result<DirectoryRecord>
where
    F: FnMut(usize) -> Result<&'a [u8]>,
{
    validate_iso_path(path)?;
    let mut current = root.clone();
    for component in path.split('/') {
        let records = read_directory(&mut read_sector, &current)?;
        let mut matches = records.into_iter().filter(|record| {
            record.is_directory && record.name.split(';').next() == Some(component)
        });
        current = matches
            .next()
            .with_context(|| format!("ISO directory component {component:?} is missing"))?;
        ensure!(
            matches.next().is_none(),
            "ISO directory component {component:?} is ambiguous"
        );
    }
    Ok(current)
}

fn extract_directory_files<'a, F>(
    mut read_sector: F,
    directory: &DirectoryRecord,
) -> Result<Vec<IsoFile>>
where
    F: FnMut(usize) -> Result<&'a [u8]>,
{
    let mut files = Vec::new();
    for record in read_directory(&mut read_sector, directory)? {
        if record.name == "." || record.name == ".." {
            continue;
        }
        ensure!(
            !record.is_directory,
            "ISO source directory contains subdirectory {}",
            record.name
        );
        let name = record
            .name
            .split(';')
            .next()
            .unwrap_or(&record.name)
            .to_owned();
        validate_file_identifier(&name)
            .with_context(|| format!("unsupported ISO source filename {name:?}"))?;
        let bytes = read_extent(&mut read_sector, record.extent_lba, record.data_length)?;
        files.push(IsoFile { name, bytes });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
    let unique = files.iter().map(|file| &file.name).collect::<BTreeSet<_>>();
    ensure!(
        unique.len() == files.len(),
        "ISO source directory has duplicate filenames"
    );
    Ok(files)
}

fn read_directory<'a, F>(
    read_sector: &mut F,
    directory: &DirectoryRecord,
) -> Result<Vec<DirectoryRecord>>
where
    F: FnMut(usize) -> Result<&'a [u8]>,
{
    ensure!(
        directory.is_directory,
        "attempted to read an ISO file as a directory"
    );
    let bytes = read_extent(read_sector, directory.extent_lba, directory.data_length)?;
    let mut records = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let record_length = bytes[offset] as usize;
        if record_length == 0 {
            offset = ((offset / LOGICAL_SECTOR_SIZE) + 1) * LOGICAL_SECTOR_SIZE;
            continue;
        }
        let end = offset
            .checked_add(record_length)
            .context("ISO directory record range overflow")?;
        ensure!(
            end <= bytes.len(),
            "ISO directory record exceeds its extent"
        );
        if let Some(record) = parse_directory_record(&bytes[offset..end])? {
            records.push(record);
        }
        offset = end;
    }
    Ok(records)
}

fn parse_directory_record(bytes: &[u8]) -> Result<Option<DirectoryRecord>> {
    if bytes.is_empty() || bytes[0] == 0 {
        return Ok(None);
    }
    let record_length = bytes[0] as usize;
    ensure!(record_length >= 34, "ISO directory record is too short");
    ensure!(
        record_length <= bytes.len(),
        "truncated ISO directory record"
    );
    let name_length = bytes[32] as usize;
    ensure!(
        33 + name_length <= record_length,
        "truncated ISO directory identifier"
    );
    let name_bytes = &bytes[33..33 + name_length];
    let name = match name_bytes {
        [0] => ".".to_owned(),
        [1] => "..".to_owned(),
        _ if name_bytes.is_ascii() => String::from_utf8(name_bytes.to_vec())?,
        _ => bail!("non-ASCII ISO directory identifier is unsupported"),
    };
    let extent_lba = read_both_endian_u32(bytes, 2, "directory extent")? as usize;
    let data_length = read_both_endian_u32(bytes, 10, "directory data length")? as usize;
    Ok(Some(DirectoryRecord {
        name,
        extent_lba,
        data_length,
        is_directory: bytes[25] & 0x02 != 0,
    }))
}

fn read_extent<'a, F>(read_sector: &mut F, start_lba: usize, data_length: usize) -> Result<Vec<u8>>
where
    F: FnMut(usize) -> Result<&'a [u8]>,
{
    let sector_count = data_length.div_ceil(LOGICAL_SECTOR_SIZE);
    let mut output = Vec::with_capacity(sector_count * LOGICAL_SECTOR_SIZE);
    for relative_lba in 0..sector_count {
        output.extend_from_slice(read_sector(start_lba + relative_lba)?);
    }
    output.truncate(data_length);
    Ok(output)
}

fn raw_user_sector(image: &[u8], lba: usize) -> Result<&[u8]> {
    let start = lba
        .checked_mul(RAW_MODE1_SECTOR_SIZE)
        .context("raw CD LBA overflow")?;
    let sector = image
        .get(start..start + RAW_MODE1_SECTOR_SIZE)
        .with_context(|| format!("raw CD image is missing LBA {lba}"))?;
    ensure!(
        sector[0] == 0
            && sector[1..11].iter().all(|byte| *byte == 0xff)
            && sector[11] == 0
            && sector[15] == 1,
        "LBA {lba} is not a raw Mode 1 sector"
    );
    Ok(&sector[RAW_MODE1_DATA_OFFSET..RAW_MODE1_DATA_OFFSET + LOGICAL_SECTOR_SIZE])
}

fn logical_sector(image: &[u8], lba: usize) -> Result<&[u8]> {
    let start = lba
        .checked_mul(LOGICAL_SECTOR_SIZE)
        .context("ISO LBA overflow")?;
    image
        .get(start..start + LOGICAL_SECTOR_SIZE)
        .with_context(|| format!("ISO image is missing LBA {lba}"))
}

fn sector_mut(image: &mut [u8], lba: usize) -> Result<&mut [u8]> {
    let start = lba
        .checked_mul(LOGICAL_SECTOR_SIZE)
        .context("ISO LBA overflow")?;
    image
        .get_mut(start..start + LOGICAL_SECTOR_SIZE)
        .with_context(|| format!("ISO image is missing writable LBA {lba}"))
}

fn write_pvd(
    sector: &mut [u8],
    volume_id: &str,
    volume_sectors: u32,
    path_table_size: u32,
    path_table_l_lba: u32,
    path_table_m_lba: u32,
    root_record: &[u8],
) -> Result<()> {
    sector.fill(0);
    sector[0] = 1;
    sector[1..6].copy_from_slice(b"CD001");
    sector[6] = 1;
    fill_ascii_field(&mut sector[8..40], "")?;
    fill_ascii_field(&mut sector[40..72], volume_id)?;
    write_both_endian_u32(&mut sector[80..88], volume_sectors);
    write_both_endian_u16(&mut sector[120..124], 1);
    write_both_endian_u16(&mut sector[124..128], 1);
    write_both_endian_u16(&mut sector[128..132], LOGICAL_SECTOR_SIZE as u16);
    write_both_endian_u32(&mut sector[132..140], path_table_size);
    sector[140..144].copy_from_slice(&path_table_l_lba.to_le_bytes());
    sector[148..152].copy_from_slice(&path_table_m_lba.to_be_bytes());
    ensure!(root_record.len() == 34, "ISO root record must be 34 bytes");
    sector[156..190].copy_from_slice(root_record);
    for field in [190..318, 318..446, 446..574, 574..702] {
        sector[field].fill(b' ');
    }
    for field in [813..830, 830..847, 847..864, 864..881] {
        sector[field].copy_from_slice(b"0000000000000000\0");
    }
    sector[881] = 1;
    Ok(())
}

fn write_volume_terminator(sector: &mut [u8]) {
    sector.fill(0);
    sector[0] = 255;
    sector[1..6].copy_from_slice(b"CD001");
    sector[6] = 1;
}

fn build_path_table(
    big_endian: bool,
    directory_name: &str,
    root_lba: usize,
    directory_lba: usize,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    write_path_record(&mut output, big_endian, &[0], root_lba, 1)?;
    write_path_record(
        &mut output,
        big_endian,
        directory_name.as_bytes(),
        directory_lba,
        1,
    )?;
    Ok(output)
}

fn write_path_record(
    output: &mut Vec<u8>,
    big_endian: bool,
    identifier: &[u8],
    extent_lba: usize,
    parent_number: u16,
) -> Result<()> {
    output.push(u8::try_from(identifier.len()).context("ISO path identifier is too long")?);
    output.push(0);
    let extent = u32::try_from(extent_lba).context("ISO path extent exceeds 32 bits")?;
    if big_endian {
        output.extend_from_slice(&extent.to_be_bytes());
        output.extend_from_slice(&parent_number.to_be_bytes());
    } else {
        output.extend_from_slice(&extent.to_le_bytes());
        output.extend_from_slice(&parent_number.to_le_bytes());
    }
    output.extend_from_slice(identifier);
    if identifier.len() % 2 == 1 {
        output.push(0);
    }
    Ok(())
}

fn directory_record_bytes(
    extent_lba: u32,
    data_length: usize,
    is_directory: bool,
    identifier: &[u8],
) -> Result<Vec<u8>> {
    let data_length =
        u32::try_from(data_length).context("ISO record data length exceeds 32 bits")?;
    let padding = usize::from(identifier.len().is_multiple_of(2));
    let length = 33usize
        .checked_add(identifier.len())
        .and_then(|value| value.checked_add(padding))
        .context("ISO directory record length overflow")?;
    ensure!(
        length <= u8::MAX as usize,
        "ISO directory record is too long"
    );
    let mut record = vec![0u8; length];
    record[0] = length as u8;
    record[1] = 0;
    write_both_endian_u32(&mut record[2..10], extent_lba);
    write_both_endian_u32(&mut record[10..18], data_length);
    record[18..25].copy_from_slice(&FIXED_RECORDING_DATE);
    record[25] = if is_directory { 0x02 } else { 0 };
    record[28] = 1;
    record[31] = 1;
    record[32] = identifier.len() as u8;
    record[33..33 + identifier.len()].copy_from_slice(identifier);
    Ok(record)
}

fn packed_directory_size(records: &[Vec<u8>]) -> Result<usize> {
    let mut offset = 0usize;
    for record in records {
        let sector_offset = offset % LOGICAL_SECTOR_SIZE;
        if sector_offset + record.len() > LOGICAL_SECTOR_SIZE {
            offset = offset
                .checked_add(LOGICAL_SECTOR_SIZE - sector_offset)
                .context("ISO directory padding overflow")?;
        }
        offset = offset
            .checked_add(record.len())
            .context("ISO directory size overflow")?;
    }
    Ok(offset.div_ceil(LOGICAL_SECTOR_SIZE) * LOGICAL_SECTOR_SIZE)
}

fn write_packed_directory(image: &mut [u8], lba: usize, records: &[Vec<u8>]) -> Result<()> {
    let mut offset = lba
        .checked_mul(LOGICAL_SECTOR_SIZE)
        .context("ISO directory LBA overflow")?;
    for record in records {
        let sector_offset = offset % LOGICAL_SECTOR_SIZE;
        if sector_offset + record.len() > LOGICAL_SECTOR_SIZE {
            offset = offset
                .checked_add(LOGICAL_SECTOR_SIZE - sector_offset)
                .context("ISO directory padding overflow")?;
        }
        let end = offset
            .checked_add(record.len())
            .context("ISO directory write overflow")?;
        image
            .get_mut(offset..end)
            .context("ISO directory write exceeds image")?
            .copy_from_slice(record);
        offset = end;
    }
    Ok(())
}

fn fill_ascii_field(field: &mut [u8], value: &str) -> Result<()> {
    ensure!(value.is_ascii(), "ISO identifier is not ASCII");
    ensure!(value.len() <= field.len(), "ISO identifier is too long");
    field.fill(b' ');
    field[..value.len()].copy_from_slice(value.as_bytes());
    Ok(())
}

fn read_both_endian_u32(bytes: &[u8], offset: usize, label: &str) -> Result<u32> {
    let little = u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .context("truncated ISO both-endian integer")?
            .try_into()
            .unwrap(),
    );
    let big = u32::from_be_bytes(
        bytes
            .get(offset + 4..offset + 8)
            .context("truncated ISO both-endian integer")?
            .try_into()
            .unwrap(),
    );
    ensure!(little == big, "ISO {label} endian copies differ");
    Ok(little)
}

fn write_both_endian_u16(bytes: &mut [u8], value: u16) {
    bytes[..2].copy_from_slice(&value.to_le_bytes());
    bytes[2..4].copy_from_slice(&value.to_be_bytes());
}

fn write_both_endian_u32(bytes: &mut [u8], value: u32) {
    bytes[..4].copy_from_slice(&value.to_le_bytes());
    bytes[4..8].copy_from_slice(&value.to_be_bytes());
}

pub(crate) fn validate_iso_path(path: &str) -> Result<()> {
    ensure!(!path.is_empty(), "ISO directory path cannot be empty");
    for component in path.split('/') {
        validate_directory_identifier(component)?;
    }
    Ok(())
}

pub(crate) fn validate_volume_id(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 32,
        "ISO volume ID must contain 1 to 32 bytes"
    );
    validate_a_characters(value, "ISO volume ID")
}

pub(crate) fn validate_directory_identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 8,
        "ISO directory identifier must contain 1 to 8 bytes"
    );
    validate_d_characters(value, "ISO directory identifier")
}

pub(crate) fn validate_file_identifier(value: &str) -> Result<()> {
    ensure!(value.is_ascii(), "ISO filename is not ASCII");
    ensure!(
        !value.contains(';') && !value.contains('/'),
        "ISO filename contains a reserved separator"
    );
    let mut parts = value.split('.');
    let base = parts.next().unwrap_or_default();
    let extension = parts.next().unwrap_or_default();
    ensure!(parts.next().is_none(), "ISO filename has more than one dot");
    ensure!(
        !base.is_empty() && base.len() <= 8,
        "ISO filename base must contain 1 to 8 bytes"
    );
    ensure!(
        extension.len() <= 3,
        "ISO filename extension must contain at most 3 bytes"
    );
    validate_d_characters(base, "ISO filename base")?;
    validate_d_characters(extension, "ISO filename extension")
}

fn validate_a_characters(value: &str, label: &str) -> Result<()> {
    ensure!(
        value.bytes().all(|byte| byte == b' '
            || byte == b'_'
            || byte.is_ascii_uppercase()
            || byte.is_ascii_digit()),
        "{label} contains characters outside ISO 9660 A-characters"
    );
    Ok(())
}

fn validate_d_characters(value: &str, label: &str) -> Result<()> {
    ensure!(
        value
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_digit()),
        "{label} contains characters outside ISO 9660 D-characters"
    );
    Ok(())
}

#[cfg(test)]
#[path = "iso9660_tests.rs"]
mod tests;
