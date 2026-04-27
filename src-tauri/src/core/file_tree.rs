use super::arena::StringArena;
use super::file_entry::{FileEntry, FileFlags};
use serde::Serialize;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

const ROOT_LARGEST_FILES_PREVIEW: usize = 8_192;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NodeSummary {
    pub id: u32,
    pub name: String,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub size: u64,
    pub child_count: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileRow {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub parent_id: u32,
    pub is_hidden: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChildPage {
    pub items: Vec<NodeSummary>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FilePathRow {
    pub id: u32,
    pub path: String,
}

#[derive(Default, Serialize)]
pub struct FileTree {
    pub entries: Vec<FileEntry>,
    pub names: StringArena,
    pub largest_files: Vec<u32>,
    pub largest_files_preview: Vec<u32>,
}

impl FileTree {
    pub fn new() -> Self {
        Self {
            entries: Vec::with_capacity(1_000_000),
            names: StringArena::new(),
            largest_files: Vec::new(),
            largest_files_preview: Vec::new(),
        }
    }

    pub fn with_root(root_name: &str) -> Self {
        Self::with_root_capacity(root_name, 1_000_000)
    }

    pub fn with_root_capacity(root_name: &str, entry_capacity: usize) -> Self {
        let mut tree = Self {
            entries: Vec::with_capacity(entry_capacity.max(1)),
            names: StringArena::new(),
            largest_files: Vec::new(),
            largest_files_preview: Vec::new(),
        };
        let (name_offset, name_len) = tree.names.push(root_name);
        tree.entries.push(FileEntry {
            size: 0,
            parent_index: FileEntry::NULL_INDEX,
            first_child_index: FileEntry::NULL_INDEX,
            next_sibling_index: FileEntry::NULL_INDEX,
            name_offset,
            name_len,
            flags: FileFlags::Directory as u16,
        });
        tree
    }

    pub fn root_id(&self) -> u32 {
        0
    }

    pub fn add_entry(&mut self, entry: FileEntry) -> u32 {
        let index = self.entries.len() as u32;
        self.entries.push(entry);
        index
    }

    pub fn attach_child(&mut self, parent_index: u32, child_index: u32) {
        if parent_index as usize >= self.entries.len() || child_index as usize >= self.entries.len()
        {
            return;
        }

        self.entries[child_index as usize].parent_index = parent_index;
        let old_first_child = self.entries[parent_index as usize].first_child_index;
        self.entries[child_index as usize].next_sibling_index = old_first_child;
        self.entries[parent_index as usize].first_child_index = child_index;
    }

    pub fn child_count(&self, id: u32) -> u32 {
        if id as usize >= self.entries.len() {
            return 0;
        }

        let mut count = 0;
        let mut child = self.entries[id as usize].first_child_index;
        while child != FileEntry::NULL_INDEX {
            count += 1;
            child = self.entries[child as usize].next_sibling_index;
        }
        count
    }

    pub fn aggregate_sizes(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.calculate_node_size(self.root_id());
    }

    fn calculate_node_size(&mut self, index: u32) -> u64 {
        let entry = self.entries[index as usize];
        if !entry.is_dir() {
            return entry.size;
        }

        let mut total: u64 = 0;
        let mut child_idx = entry.first_child_index;

        while child_idx != FileEntry::NULL_INDEX {
            total = total.saturating_add(self.calculate_node_size(child_idx));
            child_idx = self.entries[child_idx as usize].next_sibling_index;
        }

        self.entries[index as usize].size = total;
        total
    }

    pub fn rebuild_largest_files(&mut self) {
        let mut largest_files = Vec::with_capacity(self.entries.len().saturating_sub(1));
        let mut preview_heap = BinaryHeap::with_capacity(ROOT_LARGEST_FILES_PREVIEW + 1);
        for (index, entry) in self.entries.iter().enumerate() {
            if index != self.root_id() as usize && !entry.is_dir() && entry.size > 0 {
                let file_id = index as u32;
                largest_files.push(file_id);
                push_preview_candidate(&mut preview_heap, entry.size, file_id);
            }
        }

        let mut largest_files_preview: Vec<u32> = preview_heap
            .into_iter()
            .map(|candidate| candidate.id)
            .collect();
        largest_files_preview.sort_unstable_by(largest_file_order(&self.entries));

        self.largest_files = largest_files;
        self.largest_files_preview = largest_files_preview;
    }

    pub fn get_node_name(&self, id: u32) -> String {
        if id as usize >= self.entries.len() {
            return String::new();
        }

        let entry = &self.entries[id as usize];
        self.names.get(entry.name_offset, entry.name_len).to_string()
    }

    /// Returns the node name as a borrowed string to avoid allocation
    /// when the caller only needs to read the name.
    pub fn node_name_ref(&self, id: u32) -> Cow<'_, str> {
        if id as usize >= self.entries.len() {
            return Cow::Borrowed("");
        }
        let entry = &self.entries[id as usize];
        Cow::Borrowed(self.names.get(entry.name_offset, entry.name_len))
    }

    pub fn has_node(&self, id: u32) -> bool {
        (id as usize) < self.entries.len()
    }

    pub fn get_children_page(&self, id: u32, offset: usize, limit: usize) -> ChildPage {
        if !self.has_node(id) || limit == 0 {
            return ChildPage {
                items: Vec::new(),
                total: 0,
                next_offset: None,
            };
        }

        let sorted_child_ids = self.get_sorted_child_ids(id);
        self.get_children_page_from_sorted_ids(&sorted_child_ids, offset, limit)
    }

    pub fn get_sorted_child_ids(&self, id: u32) -> Vec<u32> {
        if !self.has_node(id) {
            return Vec::new();
        }

        let mut children = Vec::new();
        let mut child_idx = self.entries[id as usize].first_child_index;

        while child_idx != FileEntry::NULL_INDEX {
            let entry = &self.entries[child_idx as usize];
            children.push(ChildCandidate {
                id: child_idx,
                is_dir: entry.is_dir(),
                size: entry.size,
            });
            child_idx = entry.next_sibling_index;
        }

        sort_child_candidates(&mut children);
        children.into_iter().map(|child| child.id).collect()
    }

    pub fn get_children_page_from_sorted_ids(
        &self,
        sorted_child_ids: &[u32],
        offset: usize,
        limit: usize,
    ) -> ChildPage {
        if limit == 0 {
            return ChildPage {
                items: Vec::new(),
                total: 0,
                next_offset: None,
            };
        }

        let total = sorted_child_ids.len();
        if offset >= total {
            return ChildPage {
                items: Vec::new(),
                total,
                next_offset: None,
            };
        }

        let end = offset.saturating_add(limit).min(total);
        let items: Vec<NodeSummary> = sorted_child_ids[offset..end]
            .iter()
            .copied()
            .filter(|id| self.has_node(*id))
            .map(|id| {
                let entry = &self.entries[id as usize];
                NodeSummary {
                    id,
                    name: self.display_name(id),
                    is_dir: entry.is_dir(),
                    is_hidden: entry.is_hidden(),
                    size: entry.size,
                    child_count: u32::from(entry.first_child_index != FileEntry::NULL_INDEX),
                }
            })
            .collect();
        let consumed = end.saturating_sub(offset);
        let next_offset = (offset + consumed < total).then_some(offset + consumed);

        ChildPage {
            items,
            total,
            next_offset,
        }
    }

    pub fn get_children(&self, id: u32) -> Vec<NodeSummary> {
        self.get_children_page(id, 0, usize::MAX).items
    }

    pub fn get_file_path(&self, id: u32) -> Vec<(u32, String)> {
        // Count depth first to avoid repeated Vec growth
        let mut depth = 0usize;
        let mut current = id;
        while current != FileEntry::NULL_INDEX && (current as usize) < self.entries.len() {
            depth += 1;
            current = self.entries[current as usize].parent_index;
        }

        let mut path = Vec::with_capacity(depth);
        current = id;
        while current != FileEntry::NULL_INDEX && (current as usize) < self.entries.len() {
            let name = self.display_name(current);
            if current == self.root_id() || (!name.is_empty() && name != "\\") {
                path.push((current, name));
            }
            current = self.entries[current as usize].parent_index;
        }

        path.reverse();
        path
    }

    pub fn get_full_path(&self, id: u32) -> String {
        let path = self.get_file_path(id);
        if path.is_empty() {
            return String::new();
        }

        // Pre-calculate total length to avoid reallocations
        let total_len: usize = path
            .iter()
            .enumerate()
            .map(|(i, (_, part))| {
                if i == 0 {
                    part.len()
                } else {
                    // separator + part
                    1 + part.len()
                }
            })
            .sum();

        let mut full_path = String::with_capacity(total_len);
        for (index, (_, part)) in path.iter().enumerate() {
            if index == 0 {
                full_path.push_str(part);
                continue;
            }

            if !full_path.ends_with('\\') {
                full_path.push('\\');
            }
            full_path.push_str(part);
        }

        full_path
    }

    pub fn get_largest_files(&self, root_id: u32, offset: usize, limit: usize) -> Vec<FileRow> {
        if !self.has_node(root_id) || limit == 0 {
            return Vec::new();
        }

        let requested = offset.saturating_add(limit);
        if root_id == self.root_id()
            && requested <= self.largest_files_preview.len()
            && offset < self.largest_files_preview.len()
        {
            return self.largest_files_preview[offset..requested]
                .iter()
                .copied()
                .map(|file_id| self.file_row(file_id))
                .collect();
        }

        let file_ids = if root_id == self.root_id() {
            self.largest_files.clone()
        } else {
            self.collect_file_descendants(root_id)
        };
        self.select_largest_file_rows(file_ids, offset, limit)
    }

    fn select_largest_file_rows(
        &self,
        mut file_ids: Vec<u32>,
        offset: usize,
        limit: usize,
    ) -> Vec<FileRow> {
        let requested = offset.saturating_add(limit);
        if requested == 0 {
            return Vec::new();
        }

        if file_ids.len() > requested {
            file_ids.select_nth_unstable_by(requested, largest_file_order(&self.entries));
            file_ids.truncate(requested);
        }
        file_ids.sort_unstable_by(largest_file_order(&self.entries));

        file_ids
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|file_id| self.file_row(file_id))
            .collect()
    }

    fn collect_file_descendants(&self, root_id: u32) -> Vec<u32> {
        let mut file_ids = Vec::new();
        let mut stack = Vec::new();
        let mut child = self.entries[root_id as usize].first_child_index;

        while child != FileEntry::NULL_INDEX {
            stack.push(child);
            child = self.entries[child as usize].next_sibling_index;
        }

        while let Some(id) = stack.pop() {
            let entry = &self.entries[id as usize];
            if entry.is_dir() {
                let mut child = entry.first_child_index;
                while child != FileEntry::NULL_INDEX {
                    stack.push(child);
                    child = self.entries[child as usize].next_sibling_index;
                }
            } else if entry.size > 0 {
                file_ids.push(id);
            }
        }

        file_ids
    }

    fn file_row(&self, file_id: u32) -> FileRow {
        let entry = &self.entries[file_id as usize];
        FileRow {
            id: file_id,
            name: self.display_name(file_id),
            size: entry.size,
            parent_id: entry.parent_index,
            is_hidden: entry.is_hidden(),
        }
    }

    pub fn get_file_paths(&self, file_ids: &[u32]) -> Vec<FilePathRow> {
        let mut rows = Vec::with_capacity(file_ids.len());

        for &id in file_ids {
            if !self.has_node(id) {
                continue;
            }

            rows.push(FilePathRow {
                id,
                path: self.get_full_path(id),
            });
        }

        rows
    }

    pub fn display_name(&self, id: u32) -> String {
        if id == self.root_id() {
            return self
                .node_name_ref(id)
                .trim_end_matches('\\')
                .to_string();
        }

        let name = self.node_name_ref(id);
        if name.is_empty() || name.as_ref() == "." {
            "\\".to_string()
        } else {
            name.into_owned()
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ChildCandidate {
    id: u32,
    is_dir: bool,
    size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PreviewCandidate {
    size: u64,
    id: u32,
}

impl Ord for PreviewCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .size
            .cmp(&self.size)
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialOrd for PreviewCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn sort_child_candidates(children: &mut [ChildCandidate]) {
    children.sort_unstable_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| b.size.cmp(&a.size))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn largest_file_order(entries: &[FileEntry]) -> impl Fn(&u32, &u32) -> std::cmp::Ordering + '_ {
    |a, b| {
        entries[*b as usize]
            .size
            .cmp(&entries[*a as usize].size)
            .then_with(|| a.cmp(b))
    }
}

fn push_preview_candidate(heap: &mut BinaryHeap<PreviewCandidate>, size: u64, id: u32) {
    let candidate = PreviewCandidate { size, id };
    if heap.len() < ROOT_LARGEST_FILES_PREVIEW {
        heap.push(candidate);
        return;
    }

    let Some(worst) = heap.peek().copied() else {
        return;
    };
    if is_better_preview_candidate(candidate, worst) {
        heap.pop();
        heap.push(candidate);
    }
}

fn is_better_preview_candidate(candidate: PreviewCandidate, worst: PreviewCandidate) -> bool {
    candidate.size > worst.size || (candidate.size == worst.size && candidate.id < worst.id)
}
