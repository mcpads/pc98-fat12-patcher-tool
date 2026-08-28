use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::fat_name::FatShortName;
use crate::hash::require_sha256;
use crate::limits::MAX_FAT_DIRECTORY_DEPTH;
use crate::recipe::{ExactFile, Fat12Geometry, MountPolicy, SourceImage};

const DIRECTORY_ENTRY_BYTES: usize = 32;
const ATTRIBUTE_OFFSET: usize = 11;
const FIRST_CLUSTER_OFFSET: usize = 26;
const FILE_SIZE_OFFSET: usize = 28;
const LONG_NAME_ATTRIBUTE: u8 = 0x0f;
const VOLUME_ATTRIBUTE: u8 = 0x08;
const DIRECTORY_ATTRIBUTE: u8 = 0x10;
const END_OF_CHAIN_MINIMUM: u16 = 0x0ff8;
const END_OF_CHAIN: u16 = 0x0fff;

#[derive(Debug, Clone)]
struct Fat12Layout {
    cluster_size: usize,
    fat_offsets: Vec<usize>,
    fat_size: usize,
    root_offset: usize,
    root_bytes: usize,
    data_offset: usize,
    maximum_cluster: u16,
}

#[derive(Debug, Clone)]
struct DirectoryRecord {
    offset: usize,
    long_name_offsets: Vec<usize>,
    raw_name: [u8; 11],
    attributes: u8,
    first_cluster: u16,
    file_size: usize,
}

impl DirectoryRecord {
    fn is_directory(&self) -> bool {
        self.attributes & DIRECTORY_ATTRIBUTE != 0
    }

    fn is_volume_label(&self) -> bool {
        self.attributes & VOLUME_ATTRIBUTE != 0
    }

    fn is_dot_entry(&self) -> bool {
        self.raw_name[0] == b'.' && (self.raw_name[1] == b' ' || self.raw_name[1] == b'.')
    }
}

pub(crate) fn read_root_files(
    image: &[u8],
    _policy: MountPolicy,
    names: &BTreeSet<[u8; 11]>,
) -> Result<BTreeMap<[u8; 11], Vec<u8>>> {
    let geometry = parse_geometry(image)?;
    geometry.validate(image.len())?;
    let layout = Fat12Layout::new(&geometry, image.len())?;
    verify_fat_mirrors(image, &layout)?;
    let records = scan_directory(image, root_entry_offsets(&layout))?;
    let mut files = BTreeMap::new();
    for name in names {
        let entry = require_unique_file(&records, name)?;
        files.insert(
            *name,
            read_file(image, &layout, entry, image.len())
                .with_context(|| format!("read FAT12 file {}", raw_name_label(name)))?,
        );
    }
    Ok(files)
}

pub(crate) fn assemble_image(
    source: &[u8],
    source_profile: &SourceImage,
    retained_files: &[ExactFile],
    placed_files: &[(String, FatShortName)],
    placed_file_bytes: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>> {
    require_geometry(source, &source_profile.geometry)?;
    let layout = Fat12Layout::new(&source_profile.geometry, source.len())?;
    verify_fat_mirrors(source, &layout)?;

    let mut retained_names = BTreeSet::new();
    for file in retained_files {
        retained_names.insert(file.name.raw_bytes("retained file")?);
    }

    let mut image = source.to_vec();
    remove_nonretained_root_entries(&mut image, &layout, &retained_names)?;
    for expected in retained_files {
        verify_exact_file(&image, &layout, expected)?;
    }

    let mut allocation_hint = 2_u16;
    for (patch_key, name) in placed_files {
        let bytes = placed_file_bytes
            .get(patch_key)
            .with_context(|| format!("resolved file set is missing {patch_key}"))?;
        write_root_file(
            &mut image,
            &layout,
            name.raw_bytes("placed file name")?,
            bytes,
            &mut allocation_hint,
        )
        .with_context(|| format!("write FAT12 file {patch_key}"))?;
    }

    verify_image(
        &image,
        source_profile,
        retained_files,
        placed_files,
        placed_file_bytes,
    )?;
    Ok(image)
}

pub(crate) fn require_geometry(image: &[u8], expected: &Fat12Geometry) -> Result<()> {
    let observed = parse_geometry(image)?;
    ensure!(
        observed == *expected,
        "source FAT12 geometry differs: expected {expected:?}, got {observed:?}"
    );
    expected.validate(image.len())
}

pub(crate) fn require_fat12_structure(
    image: &[u8],
    expected: &Fat12Geometry,
    _policy: MountPolicy,
) -> Result<()> {
    require_geometry(image, expected)?;
    let layout = Fat12Layout::new(expected, image.len())?;
    verify_fat_mirrors(image, &layout)?;
    let mut claimed_clusters = BTreeSet::new();
    validate_directory(
        image,
        &layout,
        root_entry_offsets(&layout),
        0,
        &mut claimed_clusters,
    )
}

impl Fat12Layout {
    fn new(geometry: &Fat12Geometry, image_size: usize) -> Result<Self> {
        let bytes_per_sector = usize::from(geometry.bytes_per_sector);
        let cluster_size = bytes_per_sector
            .checked_mul(usize::from(geometry.sectors_per_cluster))
            .context("FAT12 cluster size overflow")?;
        let fat_size = bytes_per_sector
            .checked_mul(usize::from(geometry.sectors_per_fat))
            .context("FAT12 table size overflow")?;
        let first_fat = bytes_per_sector
            .checked_mul(usize::from(geometry.reserved_sectors))
            .context("FAT12 table offset overflow")?;
        let fat_offsets = (0..usize::from(geometry.fat_count))
            .map(|index| {
                first_fat
                    .checked_add(index.checked_mul(fat_size).context("FAT offset overflow")?)
                    .context("FAT offset overflow")
            })
            .collect::<Result<Vec<_>>>()?;
        let root_offset = first_fat
            .checked_add(
                usize::from(geometry.fat_count)
                    .checked_mul(fat_size)
                    .context("root directory offset overflow")?,
            )
            .context("root directory offset overflow")?;
        let root_bytes = usize::from(geometry.root_entries)
            .checked_mul(DIRECTORY_ENTRY_BYTES)
            .context("root directory size overflow")?;
        let root_sectors = root_bytes.div_ceil(bytes_per_sector);
        let data_offset = root_offset
            .checked_add(
                root_sectors
                    .checked_mul(bytes_per_sector)
                    .context("data offset overflow")?,
            )
            .context("data offset overflow")?;
        ensure!(data_offset < image_size, "FAT12 data area is missing");
        let data_clusters = (image_size - data_offset) / cluster_size;
        let maximum_cluster = u16::try_from(
            1_usize
                .checked_add(data_clusters)
                .context("FAT12 cluster count overflow")?,
        )
        .context("FAT12 cluster count exceeds 16-bit values")?;
        ensure!(
            maximum_cluster < 0x0ff0,
            "image has too many FAT12 clusters"
        );
        Ok(Self {
            cluster_size,
            fat_offsets,
            fat_size,
            root_offset,
            root_bytes,
            data_offset,
            maximum_cluster,
        })
    }

    fn cluster_offset(&self, cluster: u16) -> Result<usize> {
        ensure!(
            (2..=self.maximum_cluster).contains(&cluster),
            "FAT12 chain uses invalid cluster {cluster}"
        );
        self.data_offset
            .checked_add(
                usize::from(cluster - 2)
                    .checked_mul(self.cluster_size)
                    .context("cluster offset overflow")?,
            )
            .context("cluster offset overflow")
    }
}

fn parse_geometry(image: &[u8]) -> Result<Fat12Geometry> {
    ensure!(image.len() >= 28, "image is too short for a FAT12 BPB");
    let u16_at = |offset: usize| -> Result<u16> {
        let bytes: [u8; 2] = image
            .get(offset..offset + 2)
            .context("truncated FAT12 BPB")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("truncated FAT12 BPB field"))?;
        Ok(u16::from_le_bytes(bytes))
    };
    Ok(Fat12Geometry {
        bytes_per_sector: u16_at(11)?,
        sectors_per_cluster: image[13],
        reserved_sectors: u16_at(14)?,
        fat_count: image[16],
        root_entries: u16_at(17)?,
        total_sectors: u16_at(19)?,
        media_descriptor: image[21],
        sectors_per_fat: u16_at(22)?,
        sectors_per_track: u16_at(24)?,
        heads: u16_at(26)?,
    })
}

fn root_entry_offsets(layout: &Fat12Layout) -> Vec<usize> {
    (0..layout.root_bytes / DIRECTORY_ENTRY_BYTES)
        .map(|index| layout.root_offset + index * DIRECTORY_ENTRY_BYTES)
        .collect()
}

fn directory_entry_offsets(image: &[u8], layout: &Fat12Layout, cluster: u16) -> Result<Vec<usize>> {
    let chain = collect_chain_to_end(image, layout, cluster)?;
    let mut offsets = Vec::new();
    for cluster in chain {
        let start = layout.cluster_offset(cluster)?;
        for relative in (0..layout.cluster_size).step_by(DIRECTORY_ENTRY_BYTES) {
            offsets.push(start + relative);
        }
    }
    Ok(offsets)
}

fn scan_directory(image: &[u8], offsets: Vec<usize>) -> Result<Vec<DirectoryRecord>> {
    let mut records = Vec::new();
    let mut pending_long_names = Vec::new();
    for offset in offsets {
        let entry = image
            .get(offset..offset + DIRECTORY_ENTRY_BYTES)
            .context("FAT12 directory entry lies outside the image")?;
        match entry[0] {
            0x00 => break,
            0xe5 => {
                pending_long_names.clear();
                continue;
            }
            _ => {}
        }
        if entry[ATTRIBUTE_OFFSET] == LONG_NAME_ATTRIBUTE {
            pending_long_names.push(offset);
            continue;
        }
        let raw_name = entry[..11]
            .try_into()
            .map_err(|_| anyhow::anyhow!("truncated FAT12 short name"))?;
        records.push(DirectoryRecord {
            offset,
            long_name_offsets: std::mem::take(&mut pending_long_names),
            raw_name,
            attributes: entry[ATTRIBUTE_OFFSET],
            first_cluster: u16::from_le_bytes([
                entry[FIRST_CLUSTER_OFFSET],
                entry[FIRST_CLUSTER_OFFSET + 1],
            ]),
            file_size: usize::try_from(u32::from_le_bytes([
                entry[FILE_SIZE_OFFSET],
                entry[FILE_SIZE_OFFSET + 1],
                entry[FILE_SIZE_OFFSET + 2],
                entry[FILE_SIZE_OFFSET + 3],
            ]))
            .context("FAT12 file size does not fit memory")?,
        });
    }
    Ok(records)
}

fn require_unique_file<'a>(
    records: &'a [DirectoryRecord],
    expected_name: &[u8; 11],
) -> Result<&'a DirectoryRecord> {
    let mut matching = records
        .iter()
        .filter(|entry| !entry.is_volume_label() && entry.raw_name == *expected_name);
    let entry = matching.next().with_context(|| {
        format!(
            "required FAT12 root file is missing: {}",
            raw_name_label(expected_name)
        )
    })?;
    ensure!(
        matching.next().is_none(),
        "duplicate FAT12 root file: {}",
        raw_name_label(expected_name)
    );
    ensure!(
        !entry.is_directory(),
        "required FAT12 root entry is a directory: {}",
        raw_name_label(expected_name)
    );
    Ok(entry)
}

fn read_file(
    image: &[u8],
    layout: &Fat12Layout,
    entry: &DirectoryRecord,
    maximum_size: usize,
) -> Result<Vec<u8>> {
    ensure!(
        entry.file_size <= maximum_size,
        "FAT12 file {} is too large: {} bytes exceeds {maximum_size}",
        raw_name_label(&entry.raw_name),
        entry.file_size
    );
    let chain = collect_file_chain(image, layout, entry.first_cluster, entry.file_size)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(entry.file_size)
        .context("reserve FAT12 file buffer")?;
    for cluster in chain {
        let offset = layout.cluster_offset(cluster)?;
        let remaining = entry.file_size - bytes.len();
        let take = remaining.min(layout.cluster_size);
        bytes.extend_from_slice(
            image
                .get(offset..offset + take)
                .context("FAT12 data cluster lies outside the image")?,
        );
    }
    ensure!(
        bytes.len() == entry.file_size,
        "FAT12 file yielded {} bytes, expected {}",
        bytes.len(),
        entry.file_size
    );
    Ok(bytes)
}

fn collect_file_chain(
    image: &[u8],
    layout: &Fat12Layout,
    first_cluster: u16,
    file_size: usize,
) -> Result<Vec<u16>> {
    if file_size == 0 {
        ensure!(
            first_cluster == 0,
            "zero-length FAT12 file has start cluster {first_cluster}"
        );
        return Ok(Vec::new());
    }
    let expected_clusters = file_size.div_ceil(layout.cluster_size);
    let mut chain = Vec::with_capacity(expected_clusters);
    let mut visited = BTreeSet::new();
    let mut cluster = first_cluster;
    for index in 0..expected_clusters {
        ensure!(
            (2..=layout.maximum_cluster).contains(&cluster),
            "FAT12 file chain uses invalid cluster {cluster}"
        );
        ensure!(visited.insert(cluster), "FAT12 file chain contains a loop");
        chain.push(cluster);
        let next = fat_value(image, layout, cluster)?;
        if index + 1 == expected_clusters {
            ensure!(
                next >= END_OF_CHAIN_MINIMUM,
                "FAT12 file chain continues past its declared size"
            );
        } else {
            ensure!(
                (2..=layout.maximum_cluster).contains(&next),
                "FAT12 file chain ends before its declared size"
            );
            cluster = next;
        }
    }
    Ok(chain)
}

fn collect_chain_to_end(
    image: &[u8],
    layout: &Fat12Layout,
    first_cluster: u16,
) -> Result<Vec<u16>> {
    if first_cluster == 0 {
        return Ok(Vec::new());
    }
    let mut chain = Vec::new();
    let mut visited = BTreeSet::new();
    let mut cluster = first_cluster;
    loop {
        ensure!(
            (2..=layout.maximum_cluster).contains(&cluster),
            "FAT12 chain uses invalid cluster {cluster}"
        );
        ensure!(visited.insert(cluster), "FAT12 chain contains a loop");
        chain.push(cluster);
        let next = fat_value(image, layout, cluster)?;
        if next >= END_OF_CHAIN_MINIMUM {
            return Ok(chain);
        }
        ensure!(
            (2..=layout.maximum_cluster).contains(&next),
            "FAT12 chain points to invalid cluster {next}"
        );
        cluster = next;
    }
}

fn validate_directory(
    image: &[u8],
    layout: &Fat12Layout,
    offsets: Vec<usize>,
    depth: usize,
    claimed_clusters: &mut BTreeSet<u16>,
) -> Result<()> {
    ensure!(
        depth <= MAX_FAT_DIRECTORY_DEPTH,
        "FAT12 directory nesting exceeds {MAX_FAT_DIRECTORY_DEPTH} levels"
    );
    for entry in scan_directory(image, offsets)? {
        if entry.is_volume_label() || entry.is_dot_entry() {
            continue;
        }
        let chain = if entry.is_directory() {
            collect_chain_to_end(image, layout, entry.first_cluster)?
        } else {
            collect_file_chain(image, layout, entry.first_cluster, entry.file_size)?
        };
        for cluster in &chain {
            ensure!(
                claimed_clusters.insert(*cluster),
                "FAT12 entries share allocated cluster {cluster}"
            );
        }
        if entry.is_directory() {
            let next_depth = depth.checked_add(1).context("directory depth overflow")?;
            validate_directory(
                image,
                layout,
                directory_entry_offsets(image, layout, entry.first_cluster)?,
                next_depth,
                claimed_clusters,
            )?;
        }
    }
    Ok(())
}

fn remove_nonretained_root_entries(
    image: &mut [u8],
    layout: &Fat12Layout,
    retained: &BTreeSet<[u8; 11]>,
) -> Result<()> {
    let records = scan_directory(image, root_entry_offsets(layout))?;
    for entry in records {
        if entry.is_volume_label() {
            continue;
        }
        if retained.contains(&entry.raw_name) {
            ensure!(
                !entry.is_directory(),
                "retained root entry is a directory: {}",
                raw_name_label(&entry.raw_name)
            );
            continue;
        }
        if entry.is_directory() {
            remove_directory_contents(image, layout, entry.first_cluster, 1)?;
        }
        free_chain(image, layout, entry.first_cluster)?;
        mark_deleted(image, &entry)?;
    }
    Ok(())
}

fn remove_directory_contents(
    image: &mut [u8],
    layout: &Fat12Layout,
    first_cluster: u16,
    depth: usize,
) -> Result<()> {
    ensure!(
        depth <= MAX_FAT_DIRECTORY_DEPTH,
        "FAT12 directory nesting exceeds {MAX_FAT_DIRECTORY_DEPTH} levels"
    );
    let records = scan_directory(
        image,
        directory_entry_offsets(image, layout, first_cluster)?,
    )?;
    for entry in records {
        if entry.is_volume_label() || entry.is_dot_entry() {
            continue;
        }
        if entry.is_directory() {
            remove_directory_contents(
                image,
                layout,
                entry.first_cluster,
                depth.checked_add(1).context("directory depth overflow")?,
            )?;
        }
        free_chain(image, layout, entry.first_cluster)?;
        mark_deleted(image, &entry)?;
    }
    Ok(())
}

fn mark_deleted(image: &mut [u8], entry: &DirectoryRecord) -> Result<()> {
    for offset in entry
        .long_name_offsets
        .iter()
        .copied()
        .chain(std::iter::once(entry.offset))
    {
        *image
            .get_mut(offset)
            .context("FAT12 directory deletion lies outside image")? = 0xe5;
    }
    Ok(())
}

fn free_chain(image: &mut [u8], layout: &Fat12Layout, first_cluster: u16) -> Result<()> {
    for cluster in collect_chain_to_end(image, layout, first_cluster)? {
        set_fat_value(image, layout, cluster, 0)?;
    }
    Ok(())
}

fn write_root_file(
    image: &mut [u8],
    layout: &Fat12Layout,
    raw_name: [u8; 11],
    bytes: &[u8],
    allocation_hint: &mut u16,
) -> Result<()> {
    let records = scan_directory(image, root_entry_offsets(layout))?;
    ensure!(
        records
            .iter()
            .filter(|entry| !entry.is_volume_label())
            .all(|entry| entry.raw_name != raw_name),
        "FAT12 output name already exists: {}",
        raw_name_label(&raw_name)
    );
    let entry_offset = find_free_root_entry(image, layout)?;
    let cluster_count = bytes.len().div_ceil(layout.cluster_size);
    let mut clusters = Vec::with_capacity(cluster_count);
    for _ in 0..cluster_count {
        let cluster = allocate_cluster(image, layout, *allocation_hint)?;
        if let Some(previous) = clusters.last().copied() {
            set_fat_value(image, layout, previous, cluster)?;
        }
        clusters.push(cluster);
        *allocation_hint = cluster.saturating_add(1);
    }
    for (index, cluster) in clusters.iter().copied().enumerate() {
        let source_offset = index * layout.cluster_size;
        let take = (bytes.len() - source_offset).min(layout.cluster_size);
        let target_offset = layout.cluster_offset(cluster)?;
        image
            .get_mut(target_offset..target_offset + take)
            .context("allocated FAT12 cluster lies outside image")?
            .copy_from_slice(&bytes[source_offset..source_offset + take]);
    }

    let entry = image
        .get_mut(entry_offset..entry_offset + DIRECTORY_ENTRY_BYTES)
        .context("FAT12 root slot lies outside image")?;
    entry.fill(0);
    entry[..11].copy_from_slice(&raw_name);
    let first_cluster = clusters.first().copied().unwrap_or(0);
    entry[FIRST_CLUSTER_OFFSET..FIRST_CLUSTER_OFFSET + 2]
        .copy_from_slice(&first_cluster.to_le_bytes());
    let file_size = u32::try_from(bytes.len()).context("FAT12 file is larger than 4 GiB")?;
    entry[FILE_SIZE_OFFSET..FILE_SIZE_OFFSET + 4].copy_from_slice(&file_size.to_le_bytes());
    Ok(())
}

fn find_free_root_entry(image: &[u8], layout: &Fat12Layout) -> Result<usize> {
    root_entry_offsets(layout)
        .into_iter()
        .find(|offset| matches!(image.get(*offset), Some(0x00 | 0xe5)))
        .context("FAT12 root directory has no free entry")
}

fn allocate_cluster(image: &mut [u8], layout: &Fat12Layout, hint: u16) -> Result<u16> {
    let start = hint.clamp(2, layout.maximum_cluster);
    let candidates = (start..=layout.maximum_cluster).chain(2..start);
    for cluster in candidates {
        if fat_value(image, layout, cluster)? == 0 {
            set_fat_value(image, layout, cluster, END_OF_CHAIN)?;
            return Ok(cluster);
        }
    }
    anyhow::bail!("FAT12 image has no free cluster")
}

fn fat_value(image: &[u8], layout: &Fat12Layout, cluster: u16) -> Result<u16> {
    let relative = usize::from(cluster)
        .checked_add(usize::from(cluster) / 2)
        .context("FAT12 entry offset overflow")?;
    ensure!(
        relative + 2 <= layout.fat_size,
        "FAT12 entry {cluster} lies outside the table"
    );
    let offset = layout.fat_offsets[0] + relative;
    let packed = u16::from_le_bytes(
        image
            .get(offset..offset + 2)
            .context("FAT12 entry lies outside image")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("truncated FAT12 entry"))?,
    );
    Ok(if cluster.is_multiple_of(2) {
        packed & 0x0fff
    } else {
        packed >> 4
    })
}

fn set_fat_value(image: &mut [u8], layout: &Fat12Layout, cluster: u16, value: u16) -> Result<()> {
    ensure!(value <= 0x0fff, "FAT12 value exceeds 12 bits");
    let relative = usize::from(cluster)
        .checked_add(usize::from(cluster) / 2)
        .context("FAT12 entry offset overflow")?;
    ensure!(
        relative + 2 <= layout.fat_size,
        "FAT12 entry {cluster} lies outside the table"
    );
    for fat_offset in &layout.fat_offsets {
        let offset = *fat_offset + relative;
        let existing = u16::from_le_bytes(
            image
                .get(offset..offset + 2)
                .context("FAT12 entry lies outside image")?
                .try_into()
                .map_err(|_| anyhow::anyhow!("truncated FAT12 entry"))?,
        );
        let updated = if cluster.is_multiple_of(2) {
            (existing & 0xf000) | value
        } else {
            (existing & 0x000f) | (value << 4)
        };
        image
            .get_mut(offset..offset + 2)
            .context("FAT12 entry lies outside image")?
            .copy_from_slice(&updated.to_le_bytes());
    }
    Ok(())
}

fn verify_exact_file(image: &[u8], layout: &Fat12Layout, expected: &ExactFile) -> Result<()> {
    let raw_name = expected.name.raw_bytes("retained file")?;
    let records = scan_directory(image, root_entry_offsets(layout))?;
    let entry = require_unique_file(&records, &raw_name)?;
    let bytes = read_file(image, layout, entry, expected.size)?;
    ensure!(
        bytes.len() == expected.size,
        "{} size mismatch: expected {}, got {}",
        expected.name,
        expected.size,
        bytes.len()
    );
    require_sha256(&bytes, &expected.sha256, &expected.name.to_string())
}

fn verify_image(
    image: &[u8],
    source_profile: &SourceImage,
    retained_files: &[ExactFile],
    placed_files: &[(String, FatShortName)],
    placed_file_bytes: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    ensure!(
        image.len() == source_profile.size,
        "assembled image size changed: expected {}, got {}",
        source_profile.size,
        image.len()
    );
    require_fat12_structure(image, &source_profile.geometry, source_profile.mount_policy)?;
    let layout = Fat12Layout::new(&source_profile.geometry, image.len())?;
    let records = scan_directory(image, root_entry_offsets(&layout))?;
    let mut actual_names = BTreeSet::new();
    for entry in &records {
        if entry.is_volume_label() {
            continue;
        }
        ensure!(
            !entry.is_directory(),
            "assembled root contains an unexpected directory: {}",
            raw_name_label(&entry.raw_name)
        );
        ensure!(
            actual_names.insert(entry.raw_name),
            "assembled root contains a duplicate file: {}",
            raw_name_label(&entry.raw_name)
        );
    }
    let expected_names = retained_files
        .iter()
        .map(|file| file.name.raw_bytes("retained file"))
        .chain(
            placed_files
                .iter()
                .map(|(_, name)| name.raw_bytes("placed file name")),
        )
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        actual_names == expected_names,
        "assembled root file set differs: expected {}, got {}",
        raw_name_set_label(&expected_names),
        raw_name_set_label(&actual_names)
    );
    for file in retained_files {
        verify_exact_file(image, &layout, file)?;
    }
    for (patch_key, name) in placed_files {
        let expected = placed_file_bytes
            .get(patch_key)
            .with_context(|| format!("resolved file set is missing {patch_key}"))?;
        let raw_name = name.raw_bytes("placed file name")?;
        let entry = require_unique_file(&records, &raw_name)?;
        let actual = read_file(image, &layout, entry, expected.len())?;
        ensure!(actual == *expected, "assembled file differs: {patch_key}");
    }
    Ok(())
}

fn verify_fat_mirrors(image: &[u8], layout: &Fat12Layout) -> Result<()> {
    let first = image
        .get(layout.fat_offsets[0]..layout.fat_offsets[0] + layout.fat_size)
        .context("first FAT lies outside image")?;
    for (index, offset) in layout.fat_offsets.iter().enumerate().skip(1) {
        let mirror = image
            .get(*offset..*offset + layout.fat_size)
            .context("FAT mirror lies outside image")?;
        ensure!(
            mirror == first,
            "FAT mirror {index} differs from the first FAT"
        );
    }
    Ok(())
}

fn raw_name_label(name: &[u8; 11]) -> String {
    name.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn raw_name_set_label(names: &BTreeSet<[u8; 11]>) -> String {
    names
        .iter()
        .map(raw_name_label)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
#[path = "fat12_tests.rs"]
mod fat12_tests;
