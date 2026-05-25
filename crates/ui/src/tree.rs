use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ops::Range,
    rc::Rc,
};

use gpui::{
    App, Context, ElementId, Entity, EventEmitter, FocusHandle, InteractiveElement as _,
    IntoElement, KeyBinding, ListSizingBehavior, MouseButton, ParentElement, Render, RenderOnce,
    SharedString, StyleRefinement, Styled, UniformListScrollHandle, Window, div,
    prelude::FluentBuilder as _, uniform_list,
};

use crate::{
    Selectable as _, StyledExt,
    actions::{Confirm, SelectDown, SelectLeft, SelectRight, SelectUp},
    list::ListItem,
    menu::{ContextMenuExt as _, PopupMenu},
    scroll::ScrollableElement,
};

const CONTEXT: &str = "Tree";

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
    ]);
}

/// Create a [`Tree`].
///
/// # Arguments
///
/// * `state` - The shared state managing the tree items.
/// * `render_item` - A closure to render each visible tree item.
pub fn tree<R>(state: &Entity<TreeState>, render_item: R) -> Tree
where
    R: Fn(usize, &TreeEntry, bool, &mut Window, &mut App) -> ListItem + 'static,
{
    Tree::new(state, render_item)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEvent {
    Select(SharedString),
    Activate(SharedString),
    Expand(SharedString),
    Collapse(SharedString),
    ContextMenu(SharedString),
}

#[derive(Clone)]
struct TreeItemState {
    expanded: bool,
    disabled: bool,
    branch: bool,
    loading: bool,
}

/// A tree item used to seed or update [`TreeState`].
///
/// `TreeState` normalizes these items into an internal id-indexed model. Visible
/// rows keep only shallow item snapshots, so rendering and expansion no longer
/// clone whole subtrees.
#[derive(Clone)]
pub struct TreeItem {
    pub id: SharedString,
    pub label: SharedString,
    pub children: Vec<TreeItem>,
    state: Rc<RefCell<TreeItemState>>,
}

#[derive(Clone)]
struct TreeNode {
    id: SharedString,
    label: SharedString,
    parent: Option<String>,
    children: Vec<String>,
    expanded: bool,
    disabled: bool,
    branch: bool,
    loading: bool,
}

/// A flat representation of a visible tree item with its depth.
#[derive(Clone)]
pub struct TreeEntry {
    item: TreeItem,
    depth: usize,
}

impl TreeEntry {
    fn new(node: &TreeNode, depth: usize) -> Self {
        Self {
            item: TreeItem::from_node(node),
            depth,
        }
    }

    /// Get the shallow source tree item for this visible row.
    #[inline]
    pub fn item(&self) -> &TreeItem {
        &self.item
    }

    #[inline]
    pub fn id(&self) -> &SharedString {
        &self.item.id
    }

    #[inline]
    pub fn label(&self) -> &SharedString {
        &self.item.label
    }

    /// The depth of this item in the tree.
    #[inline]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Whether this item is expandable.
    #[inline]
    pub fn is_folder(&self) -> bool {
        self.item.is_folder()
    }

    /// Whether this item is expanded.
    #[inline]
    pub fn is_expanded(&self) -> bool {
        self.item.is_expanded()
    }

    #[inline]
    pub fn is_disabled(&self) -> bool {
        self.item.is_disabled()
    }

    #[inline]
    pub fn is_loading(&self) -> bool {
        self.item.is_loading()
    }

    fn id_key(&self) -> String {
        self.item.id.to_string()
    }
}

impl TreeItem {
    /// Create a new tree item with the given id and label.
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            children: Vec::new(),
            state: Rc::new(RefCell::new(TreeItemState {
                expanded: false,
                disabled: false,
                branch: false,
                loading: false,
            })),
        }
    }

    fn from_node(node: &TreeNode) -> Self {
        Self {
            id: node.id.clone(),
            label: node.label.clone(),
            children: Vec::new(),
            state: Rc::new(RefCell::new(TreeItemState {
                expanded: node.expanded,
                disabled: node.disabled,
                branch: node.branch,
                loading: node.loading,
            })),
        }
    }

    /// Add a child item.
    pub fn child(mut self, child: TreeItem) -> Self {
        self.state.borrow_mut().branch = true;
        self.children.push(child);
        self
    }

    /// Add multiple child items.
    pub fn children(mut self, children: impl IntoIterator<Item = TreeItem>) -> Self {
        self.children.extend(children);
        if !self.children.is_empty() {
            self.state.borrow_mut().branch = true;
        }
        self
    }

    /// Mark whether this item can expand even when children are not loaded yet.
    pub fn branch(self, branch: bool) -> Self {
        self.state.borrow_mut().branch = branch;
        self
    }

    /// Alias for [`TreeItem::branch`] for file-tree style call sites.
    pub fn folder(self, folder: bool) -> Self {
        self.branch(folder)
    }

    /// Set expanded state for this item.
    pub fn expanded(self, expanded: bool) -> Self {
        self.state.borrow_mut().expanded = expanded;
        self
    }

    /// Set disabled state for this item.
    pub fn disabled(self, disabled: bool) -> Self {
        self.state.borrow_mut().disabled = disabled;
        self
    }

    /// Set loading state for this item.
    pub fn loading(self, loading: bool) -> Self {
        self.state.borrow_mut().loading = loading;
        self
    }

    /// Whether this item is expandable.
    #[inline]
    pub fn is_folder(&self) -> bool {
        self.state.borrow().branch || !self.children.is_empty()
    }

    /// Whether this item is disabled.
    #[inline]
    pub fn is_disabled(&self) -> bool {
        self.state.borrow().disabled
    }

    /// Whether this item is expanded.
    #[inline]
    pub fn is_expanded(&self) -> bool {
        self.state.borrow().expanded
    }

    /// Whether this item is currently loading its children.
    #[inline]
    pub fn is_loading(&self) -> bool {
        self.state.borrow().loading
    }
}

/// State for managing tree items.
pub struct TreeState {
    focus_handle: FocusHandle,
    roots: Vec<String>,
    nodes: HashMap<String, TreeNode>,
    entries: Vec<TreeEntry>,
    scroll_handle: UniformListScrollHandle,
    selected_id: Option<String>,
    right_clicked_id: Option<String>,
    render_item: Rc<dyn Fn(usize, &TreeEntry, bool, &mut Window, &mut App) -> ListItem>,
    context_menu_builder: Option<
        Rc<dyn Fn(usize, &TreeEntry, PopupMenu, &mut Window, &mut Context<TreeState>) -> PopupMenu>,
    >,
}

impl EventEmitter<TreeEvent> for TreeState {}

impl TreeState {
    /// Create a new empty tree state.
    pub fn new(cx: &mut App) -> Self {
        Self {
            selected_id: None,
            right_clicked_id: None,
            focus_handle: cx.focus_handle(),
            scroll_handle: UniformListScrollHandle::default(),
            roots: Vec::new(),
            nodes: HashMap::new(),
            entries: Vec::new(),
            render_item: Rc::new(|_, _, _, _, _| ListItem::new(0)),
            context_menu_builder: None,
        }
    }

    /// Set the tree items.
    pub fn items(mut self, items: impl Into<Vec<TreeItem>>) -> Self {
        self.replace_roots(items.into());
        self
    }

    /// Replace all root items.
    pub fn set_items(&mut self, items: impl Into<Vec<TreeItem>>, cx: &mut Context<Self>) {
        self.replace_roots(items.into());
        cx.notify();
    }

    /// Replace children for an existing node without rebuilding unrelated rows.
    pub fn replace_children(
        &mut self,
        parent_id: impl AsRef<str>,
        children: impl Into<Vec<TreeItem>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let parent_id = parent_id.as_ref().to_owned();
        if !self.nodes.contains_key(&parent_id) {
            return false;
        }

        let old_children = self
            .nodes
            .get(&parent_id)
            .map_or_else(Vec::new, |node| node.children.clone());
        let mut old_descendants = HashSet::new();
        self.collect_descendant_ids(&old_children, &mut old_descendants);
        for id in old_descendants {
            self.nodes.remove(&id);
        }

        let child_ids = children
            .into()
            .into_iter()
            .map(|child| self.insert_item(Some(parent_id.clone()), child))
            .collect::<Vec<_>>();

        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.children = child_ids;
            parent.branch = true;
            parent.loading = false;
        }

        self.replace_visible_descendants(&parent_id);
        self.retain_live_state();
        cx.notify();
        true
    }

    /// Set loading state for a node.
    pub fn set_loading(
        &mut self,
        id: impl AsRef<str>,
        loading: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let id = id.as_ref();
        let Some(node) = self.nodes.get_mut(id) else {
            return false;
        };

        if node.loading == loading {
            return false;
        }

        node.loading = loading;
        self.refresh_visible_entry(id);
        cx.notify();
        true
    }

    /// Get the currently selected index, if any.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_id
            .as_deref()
            .and_then(|id| self.position_of(id))
    }

    /// Get the currently selected id, if any.
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    /// Set the selected index, or `None` to clear selection.
    pub fn set_selected_index(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        self.selected_id = ix.and_then(|ix| self.entries.get(ix).map(TreeEntry::id_key));
        cx.notify();
    }

    /// Select a node by id and reveal its ancestors when possible.
    pub fn set_selected_id(&mut self, id: Option<&str>, cx: &mut Context<Self>) {
        self.selected_id = id
            .map(ToOwned::to_owned)
            .filter(|id| self.nodes.contains_key(id));
        if let Some(id) = self.selected_id.clone() {
            self.expand_ancestors(&id);
        }
        self.rebuild_entries();
        cx.notify();
    }

    /// Set the selected item, or `None` to clear selection.
    pub fn set_selected_item(&mut self, item: Option<&TreeItem>, cx: &mut Context<Self>) {
        self.set_selected_id(item.map(|item| item.id.as_str()), cx);
    }

    /// Get the currently selected tree item, if any.
    pub fn selected_item(&self) -> Option<&TreeItem> {
        self.selected_index()
            .and_then(|ix| self.entries.get(ix).map(|entry| &entry.item))
    }

    /// Get the currently selected entry, if any.
    pub fn selected_entry(&self) -> Option<&TreeEntry> {
        self.selected_index().and_then(|ix| self.entries.get(ix))
    }

    pub fn entry(&self, ix: usize) -> Option<&TreeEntry> {
        self.entries.get(ix)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn scroll_to_item(&mut self, ix: usize, strategy: gpui::ScrollStrategy) {
        self.scroll_handle.scroll_to_item(ix, strategy);
    }

    /// Set expansion state for a node.
    pub fn set_expanded(
        &mut self,
        id: impl AsRef<str>,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.set_expanded_by_key(id.as_ref(), expanded, true, cx)
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut App) {
        self.focus_handle.focus(window, cx);
    }

    fn replace_roots(&mut self, items: Vec<TreeItem>) {
        self.nodes.clear();
        self.roots.clear();
        self.entries.clear();

        self.roots = items
            .into_iter()
            .map(|item| self.insert_item(None, item))
            .collect();
        self.rebuild_entries();
        self.retain_live_state();
    }

    fn insert_item(&mut self, parent: Option<String>, item: TreeItem) -> String {
        let TreeItem {
            id,
            label,
            children,
            state,
        } = item;
        let item_state = state.borrow().clone();
        let branch = item_state.branch || !children.is_empty();
        let id_key = id.to_string();

        self.nodes.insert(
            id_key.clone(),
            TreeNode {
                id,
                label,
                parent,
                children: Vec::new(),
                expanded: item_state.expanded,
                disabled: item_state.disabled,
                branch,
                loading: item_state.loading,
            },
        );

        let child_ids = children
            .into_iter()
            .map(|child| self.insert_item(Some(id_key.clone()), child))
            .collect::<Vec<_>>();

        if let Some(node) = self.nodes.get_mut(&id_key) {
            node.children = child_ids;
        }

        id_key
    }

    fn collect_descendant_ids(&self, ids: &[String], output: &mut HashSet<String>) {
        for id in ids {
            if !output.insert(id.clone()) {
                continue;
            }

            if let Some(node) = self.nodes.get(id) {
                self.collect_descendant_ids(&node.children, output);
            }
        }
    }

    fn rebuild_entries(&mut self) {
        let roots = self.roots.clone();
        self.entries.clear();
        self.entries = self.visible_entries_for_children(&roots, 0);
    }

    fn visible_entries_for_children(&self, ids: &[String], depth: usize) -> Vec<TreeEntry> {
        let mut entries = Vec::new();
        for id in ids {
            self.push_visible_entries(id, depth, &mut entries);
        }
        entries
    }

    fn push_visible_entries(&self, id: &str, depth: usize, entries: &mut Vec<TreeEntry>) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };

        entries.push(TreeEntry::new(node, depth));
        if node.expanded {
            for child_id in &node.children {
                self.push_visible_entries(child_id, depth + 1, entries);
            }
        }
    }

    fn position_of(&self, id: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.item.id.as_str() == id)
    }

    fn visible_subtree_end(&self, ix: usize) -> usize {
        let Some(parent) = self.entries.get(ix) else {
            return ix;
        };
        let parent_depth = parent.depth;
        self.entries
            .iter()
            .enumerate()
            .skip(ix + 1)
            .find_map(|(entry_ix, entry)| (entry.depth <= parent_depth).then_some(entry_ix))
            .unwrap_or(self.entries.len())
    }

    fn replace_visible_descendants(&mut self, parent_id: &str) {
        let Some(parent_ix) = self.position_of(parent_id) else {
            return;
        };
        let parent_depth = self.entries[parent_ix].depth;
        let end_ix = self.visible_subtree_end(parent_ix);
        self.entries.drain(parent_ix + 1..end_ix);

        if let Some(parent) = self.nodes.get(parent_id) {
            self.entries[parent_ix] = TreeEntry::new(parent, parent_depth);
            if parent.expanded {
                let descendants =
                    self.visible_entries_for_children(&parent.children, parent_depth + 1);
                self.entries
                    .splice(parent_ix + 1..parent_ix + 1, descendants);
            }
        }
    }

    fn refresh_visible_entry(&mut self, id: &str) {
        let Some(ix) = self.position_of(id) else {
            return;
        };
        let depth = self.entries[ix].depth;
        if let Some(node) = self.nodes.get(id) {
            self.entries[ix] = TreeEntry::new(node, depth);
        }
    }

    fn expand_ancestors(&mut self, id: &str) {
        let mut parent = self.nodes.get(id).and_then(|node| node.parent.clone());
        while let Some(parent_id) = parent {
            parent = self
                .nodes
                .get(&parent_id)
                .and_then(|node| node.parent.clone());
            if let Some(node) = self.nodes.get_mut(&parent_id) {
                node.expanded = true;
            }
        }
    }

    fn retain_live_state(&mut self) {
        if self
            .selected_id
            .as_ref()
            .is_some_and(|id| !self.nodes.contains_key(id))
        {
            self.selected_id = None;
        }
        if self
            .right_clicked_id
            .as_ref()
            .is_some_and(|id| !self.nodes.contains_key(id))
        {
            self.right_clicked_id = None;
        }
    }

    fn set_expanded_by_key(
        &mut self,
        id: &str,
        expanded: bool,
        emit_event: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(node) = self.nodes.get_mut(id) else {
            return false;
        };

        if !node.branch || node.expanded == expanded {
            return false;
        }

        node.expanded = expanded;
        let event_id = node.id.clone();

        if let Some(ix) = self.position_of(id) {
            let depth = self.entries[ix].depth;
            if let Some(node) = self.nodes.get(id) {
                self.entries[ix] = TreeEntry::new(node, depth);
            }

            if expanded {
                if let Some(node) = self.nodes.get(id) {
                    let descendants = self.visible_entries_for_children(&node.children, depth + 1);
                    self.entries.splice(ix + 1..ix + 1, descendants);
                }
            } else {
                let end_ix = self.visible_subtree_end(ix);
                self.entries.drain(ix + 1..end_ix);
            }
        }

        if emit_event {
            if expanded {
                cx.emit(TreeEvent::Expand(event_id));
            } else {
                cx.emit(TreeEvent::Collapse(event_id));
            }
        }
        cx.notify();
        true
    }

    fn on_action_confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_ix) = self.selected_index() else {
            return;
        };

        if let Some(entry) = self.entries.get(selected_ix).cloned() {
            if entry.is_folder() {
                let id = entry.id_key();
                let expanded = self.nodes.get(&id).is_some_and(|node| node.expanded);
                self.set_expanded_by_key(&id, !expanded, true, cx);
            } else {
                cx.emit(TreeEvent::Activate(entry.item.id.clone()));
            }
        }
    }

    fn on_action_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_ix) = self.selected_index() else {
            return;
        };

        if let Some(entry) = self.entries.get(selected_ix).cloned()
            && entry.is_folder()
            && entry.is_expanded()
        {
            self.set_expanded_by_key(&entry.id_key(), false, true, cx);
        }
    }

    fn on_action_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_ix) = self.selected_index() else {
            return;
        };

        if let Some(entry) = self.entries.get(selected_ix).cloned()
            && entry.is_folder()
            && !entry.is_expanded()
        {
            self.set_expanded_by_key(&entry.id_key(), true, true, cx);
        }
    }

    fn on_action_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let mut selected_ix = self.selected_index().unwrap_or(0);

        if selected_ix > 0 {
            selected_ix -= 1;
        } else {
            selected_ix = self.entries.len().saturating_sub(1);
        }

        if let Some(entry) = self.entries.get(selected_ix) {
            self.selected_id = Some(entry.id_key());
            cx.emit(TreeEvent::Select(entry.item.id.clone()));
        }
        cx.notify();
    }

    fn on_action_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let mut selected_ix = self.selected_index().unwrap_or(0);
        if selected_ix + 1 < self.entries.len() {
            selected_ix += 1;
        } else {
            selected_ix = 0;
        }

        if let Some(entry) = self.entries.get(selected_ix) {
            self.selected_id = Some(entry.id_key());
            cx.emit(TreeEvent::Select(entry.item.id.clone()));
        }
        cx.notify();
    }

    fn on_entry_click(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(ix).cloned() else {
            return;
        };

        self.selected_id = Some(entry.id_key());
        cx.emit(TreeEvent::Select(entry.item.id.clone()));

        if entry.is_folder() {
            let id = entry.id_key();
            let expanded = self.nodes.get(&id).is_some_and(|node| node.expanded);
            self.set_expanded_by_key(&id, !expanded, true, cx);
        } else {
            cx.emit(TreeEvent::Activate(entry.item.id.clone()));
            cx.notify();
        }
    }
}

impl Render for TreeState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_item = self.render_item.clone();
        let state = cx.entity().clone();

        div()
            .id("tree-state")
            .size_full()
            .relative()
            .context_menu({
                let state = state.clone();
                move |menu, window, cx: &mut Context<PopupMenu>| {
                    if state.read(cx).context_menu_builder.is_none() {
                        return menu;
                    }

                    let (ix, entry) = {
                        let state = state.read(cx);
                        let ix = state
                            .right_clicked_id
                            .as_deref()
                            .and_then(|id| state.position_of(id));
                        let entry = ix.and_then(|ix| state.entries.get(ix).cloned());
                        (ix, entry)
                    };

                    if let (Some(ix), Some(entry)) = (ix, entry) {
                        state.update(cx, |state, cx| {
                            if let Some(build) = state.context_menu_builder.clone() {
                                build(ix, &entry, menu, window, cx)
                            } else {
                                menu
                            }
                        })
                    } else {
                        menu
                    }
                }
            })
            .child(
                uniform_list("entries", self.entries.len(), {
                    cx.processor(move |state, visible_range: Range<usize>, window, cx| {
                        let mut items = Vec::with_capacity(visible_range.len());
                        for ix in visible_range {
                            let entry = &state.entries[ix];
                            let selected = state
                                .selected_id
                                .as_deref()
                                .is_some_and(|id| id == entry.item.id.as_str());
                            let right_clicked = state
                                .right_clicked_id
                                .as_deref()
                                .is_some_and(|id| id == entry.item.id.as_str());
                            let item = (render_item)(ix, entry, selected, window, cx);

                            let el = div()
                                .id(ix)
                                .child(
                                    item.disabled(entry.item().is_disabled())
                                        .selected(selected)
                                        .secondary_selected(right_clicked),
                                )
                                .when(!entry.item().is_disabled(), |this| {
                                    this.on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, window, cx| {
                                            this.on_entry_click(ix, window, cx);
                                        }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |this, _, _, cx| {
                                            let Some(entry) = this.entries.get(ix) else {
                                                return;
                                            };
                                            this.right_clicked_id = Some(entry.id_key());
                                            cx.emit(TreeEvent::ContextMenu(entry.item.id.clone()));
                                            cx.notify();
                                        }),
                                    )
                                });

                            items.push(el)
                        }

                        items
                    })
                })
                .flex_grow()
                .size_full()
                .track_scroll(&self.scroll_handle)
                .with_sizing_behavior(ListSizingBehavior::Auto)
                .into_any_element(),
            )
    }
}

/// A tree view element that displays hierarchical data.
#[derive(IntoElement)]
pub struct Tree {
    id: ElementId,
    state: Entity<TreeState>,
    style: StyleRefinement,
    render_item: Rc<dyn Fn(usize, &TreeEntry, bool, &mut Window, &mut App) -> ListItem>,
    context_menu_builder: Option<
        Rc<dyn Fn(usize, &TreeEntry, PopupMenu, &mut Window, &mut Context<TreeState>) -> PopupMenu>,
    >,
}

impl Tree {
    pub fn new<R>(state: &Entity<TreeState>, render_item: R) -> Self
    where
        R: Fn(usize, &TreeEntry, bool, &mut Window, &mut App) -> ListItem + 'static,
    {
        Self {
            id: ElementId::Name(format!("tree-{}", state.entity_id()).into()),
            state: state.clone(),
            style: StyleRefinement::default(),
            render_item: Rc::new(move |ix, item, selected, window, app| {
                render_item(ix, item, selected, window, app)
            }),
            context_menu_builder: None,
        }
    }

    /// Add a context menu to the tree.
    pub fn context_menu<F>(mut self, f: F) -> Self
    where
        F: Fn(usize, &TreeEntry, PopupMenu, &mut Window, &mut Context<TreeState>) -> PopupMenu
            + 'static,
    {
        self.context_menu_builder = Some(Rc::new(f));
        self
    }
}

impl Styled for Tree {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Tree {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus_handle = self.state.read(cx).focus_handle.clone();
        let scroll_handle = self.state.read(cx).scroll_handle.clone();

        self.state.update(cx, |state, _| {
            state.render_item = self.render_item;
            state.context_menu_builder = self.context_menu_builder;
        });

        div()
            .id(self.id)
            .key_context(CONTEXT)
            .track_focus(&focus_handle)
            .on_action(window.listener_for(&self.state, TreeState::on_action_confirm))
            .on_action(window.listener_for(&self.state, TreeState::on_action_left))
            .on_action(window.listener_for(&self.state, TreeState::on_action_right))
            .on_action(window.listener_for(&self.state, TreeState::on_action_up))
            .on_action(window.listener_for(&self.state, TreeState::on_action_down))
            .size_full()
            .child(self.state)
            .refine_style(&self.style)
            .vertical_scrollbar(&scroll_handle)
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::{TreeItem, TreeState};
    use gpui::AppContext as _;

    fn assert_entries(entries: &[super::TreeEntry], expected: &str) {
        let actual = entries
            .iter()
            .map(|entry| {
                let mut row = String::new();
                row.push_str(&"    ".repeat(entry.depth));
                row.push_str(entry.item().label.as_str());
                row
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(actual.trim(), expected.trim());
    }

    #[gpui::test]
    fn tree_flattens_expanded_items_without_deep_row_children(cx: &mut gpui::TestAppContext) {
        let items = vec![
            TreeItem::new("src", "src")
                .expanded(true)
                .child(
                    TreeItem::new("src/ui", "ui")
                        .expanded(true)
                        .child(TreeItem::new("src/ui/button.rs", "button.rs"))
                        .child(TreeItem::new("src/ui/icon.rs", "icon.rs"))
                        .child(TreeItem::new("src/ui/mod.rs", "mod.rs")),
                )
                .child(TreeItem::new("src/lib.rs", "lib.rs")),
            TreeItem::new("Cargo.toml", "Cargo.toml"),
            TreeItem::new("Cargo.lock", "Cargo.lock").disabled(true),
            TreeItem::new("README.md", "README.md"),
        ];

        let state = cx.new(|cx| TreeState::new(cx).items(items));
        state.update(cx, |state, cx| {
            assert_entries(
                &state.entries,
                indoc! {
                    r#"
                    src
                        ui
                            button.rs
                            icon.rs
                            mod.rs
                        lib.rs
                    Cargo.toml
                    Cargo.lock
                    README.md
                    "#
                },
            );

            let entry = state.entries.get(1).expect("ui entry");
            assert!(entry.is_folder());
            assert!(entry.is_expanded());
            assert!(entry.item().children.is_empty());

            state.set_expanded("src/ui", false, cx);
            assert_entries(
                &state.entries,
                indoc! {
                    r#"
                    src
                        ui
                        lib.rs
                    Cargo.toml
                    Cargo.lock
                    README.md
                    "#
                },
            );
        })
    }

    #[gpui::test]
    fn branch_items_can_expand_before_children_are_loaded(cx: &mut gpui::TestAppContext) {
        let items = vec![TreeItem::new("chapters", "chapters").branch(true)];

        let state = cx.new(|cx| TreeState::new(cx).items(items));
        state.update(cx, |state, cx| {
            assert_eq!(state.len(), 1);
            assert!(state.entries[0].is_folder());
            assert!(state.set_expanded("chapters", true, cx));
            assert_eq!(state.len(), 1);

            assert!(state.replace_children(
                "chapters",
                vec![TreeItem::new("chapters/intro.tex", "intro.tex")],
                cx,
            ));
            assert_entries(
                &state.entries,
                indoc! {
                    r#"
                    chapters
                        intro.tex
                    "#
                },
            );
        });
    }

    #[gpui::test]
    fn selection_is_preserved_by_id_across_root_replacement(cx: &mut gpui::TestAppContext) {
        let state = cx.new(|cx| {
            TreeState::new(cx).items(vec![TreeItem::new("a", "a"), TreeItem::new("b", "b")])
        });

        state.update(cx, |state, cx| {
            state.set_selected_id(Some("b"), cx);
            state.set_items(
                vec![TreeItem::new("b", "b renamed"), TreeItem::new("c", "c")],
                cx,
            );

            assert_eq!(state.selected_id(), Some("b"));
            assert_eq!(
                state.selected_entry().map(|entry| entry.label().as_str()),
                Some("b renamed")
            );
        });
    }
}
