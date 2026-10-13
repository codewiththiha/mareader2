//! Where a destination leads: a page, found directly or by name.

use std::collections::HashMap;

use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::dict::keys::{A, D, DEST, DESTS, KIDS, NAMES, S};
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier};

/// Name tree levels walked before giving up on a malformed file.
const MAX_TREE_DEPTH: usize = 32;

/// Each page object's 0-based index, built once when a file opens.
pub(crate) type PageIds = HashMap<ObjectIdentifier, usize>;

pub(crate) fn page_ids(pdf: &Pdf) -> PageIds {
    pdf.pages()
        .iter()
        .enumerate()
        .filter_map(|(i, p)| Some((p.raw().obj_id()?, i)))
        .collect()
}

/// Where destinations point: page objects and the named tables.
pub(crate) struct Targets<'a> {
    pub catalog: Dict<'a>,
    pages: &'a PageIds,
    count: usize,
    /// PDF 1.1's `/Dests` dictionary, keyed by name.
    old_names: Option<Dict<'a>>,
    /// The `/Names` → `/Dests` tree, keyed by string.
    tree: Option<Dict<'a>>,
}

impl<'a> Targets<'a> {
    pub(crate) fn new(pdf: &'a Pdf, pages: &'a PageIds) -> Option<Self> {
        let xref = pdf.xref();
        let catalog = xref.get::<Dict<'a>>(xref.root_id())?;
        Some(Self {
            pages,
            count: pdf.pages().len(),
            old_names: catalog.get::<Dict<'a>>(DESTS),
            tree: catalog
                .get::<Dict<'a>>(NAMES)
                .and_then(|n| n.get::<Dict<'a>>(DESTS)),
            catalog,
        })
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// A node's `/Dest`, or the target of its GoTo action.
    pub(crate) fn of_node(&self, node: &Dict<'_>) -> Option<usize> {
        if let Some(dest) = node.get::<Object<'_>>(DEST) {
            return self.of_dest(dest, 0);
        }
        let action = node.get::<Dict<'_>>(A)?;
        let goto = action
            .get::<Name<'_>>(S)
            .is_some_and(|s| s.as_ref() == b"GoTo");
        goto.then(|| action.get::<Object<'_>>(D))
            .flatten()
            .and_then(|d| self.of_dest(d, 0))
    }

    /// An explicit array, a name or string to look up, or `<< /D … >>`.
    fn of_dest(&self, dest: Object<'_>, hops: usize) -> Option<usize> {
        if hops > 2 {
            return None;
        }
        match dest {
            Object::Array(a) => self.of_array(&a),
            Object::Dict(d) => self.of_dest(d.get::<Object<'_>>(D)?, hops + 1),
            Object::Name(n) => {
                let found = self.old_names.as_ref()?.get::<Object<'_>>(n.as_ref())?;
                self.of_dest(found, hops + 1)
            }
            Object::String(s) => {
                let found = lookup(self.tree.as_ref()?, s.as_ref(), 0)?;
                self.of_dest(found, hops + 1)
            }
            _ => None,
        }
    }

    /// `[page /XYZ …]`: a page object here, or a number in remote files.
    fn of_array(&self, array: &Array<'_>) -> Option<usize> {
        let page = match array.raw_iter().next()? {
            MaybeRef::Ref(r) => *self.pages.get(&ObjectIdentifier::from(r))?,
            MaybeRef::NotRef(o) => usize::try_from(o.into_i32()?).ok()?,
        };
        (page < self.count).then_some(page)
    }
}

/// Finds `key` in a name tree, steering by each kid's `/Limits`.
fn lookup<'a>(node: &Dict<'a>, key: &[u8], depth: usize) -> Option<Object<'a>> {
    if depth > MAX_TREE_DEPTH {
        return None;
    }
    if let Some(names) = node.get::<Array<'a>>(NAMES) {
        let mut items = names.iter::<Object<'a>>();
        while let (Some(k), Some(v)) = (items.next(), items.next()) {
            if let Object::String(s) = k
                && s.as_ref() == key
            {
                return Some(v);
            }
        }
    }
    let kids = node.get::<Array<'a>>(KIDS)?;
    kids.iter::<Dict<'a>>()
        .filter(|kid| covers(kid, key))
        .find_map(|kid| lookup(&kid, key, depth + 1))
}

/// Whether a kid's `/Limits` may hold `key`; no limits means maybe.
fn covers(kid: &Dict<'_>, key: &[u8]) -> bool {
    use hayro::hayro_syntax::object::String as PdfString;
    let Some(limits) = kid.get::<Array<'_>>(b"Limits") else {
        return true;
    };
    let mut ends = limits.iter::<PdfString<'_>>();
    match (ends.next(), ends.next()) {
        (Some(lo), Some(hi)) => lo.as_ref() <= key && key <= hi.as_ref(),
        _ => true,
    }
}
