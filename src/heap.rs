use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    sync::Arc,
};

use crate::syntax::Literal;

pub(crate) type Memory = Rc<RefCell<Vec<Value>>>;

#[derive(Clone, Debug)]
pub(crate) enum Value {
    Int(i32),
    Bool(bool),
    Char(u8),
    String(Arc<[u8]>),
    Pointer(Option<Memory>),
    Array(Option<Memory>),
    Struct(String, Memory),
    Resource(usize),
    Window(u64),
    Float(f64),
    Void,
}

impl From<&Literal> for Value {
    fn from(literal: &Literal) -> Self {
        match literal {
            Literal::Int(value) => Self::Int(*value),
            Literal::Bool(value) => Self::Bool(*value),
            Literal::Char(value) => Self::Char(*value),
            Literal::String(value) => Self::String(Arc::clone(value)),
            Literal::Null => Self::Pointer(None),
        }
    }
}

impl Value {
    pub(crate) fn string(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::String(bytes.into())
    }

    fn memory(&self) -> Option<&Memory> {
        match self {
            Self::Pointer(memory) | Self::Array(memory) => memory.as_ref(),
            Self::Struct(_, memory) => Some(memory),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct Heap {
    blocks: Vec<Weak<RefCell<Vec<Value>>>>,
    next_collection: usize,
}

impl Heap {
    pub(crate) fn block(&mut self, values: Vec<Value>) -> Memory {
        if self.blocks.len() >= self.next_collection {
            self.collect();
            self.next_collection = self.blocks.len().saturating_mul(2).max(256);
        }
        let memory = Rc::new(RefCell::new(values));
        self.blocks.push(Rc::downgrade(&memory));
        memory
    }

    fn collect(&mut self) {
        use std::collections::HashMap;

        let blocks: Vec<_> = self.blocks.iter().filter_map(Weak::upgrade).collect();
        let indices: HashMap<_, _> = blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (Rc::as_ptr(block), index))
            .collect();
        let mut incoming = vec![1; blocks.len()];
        for block in &blocks {
            for child in block.borrow().iter().filter_map(Value::memory) {
                incoming[indices[&Rc::as_ptr(child)]] += 1;
            }
        }

        // The remaining strong references come from frames, temporaries, or host resources.
        let mut marked = vec![false; blocks.len()];
        let mut pending: Vec<_> = blocks
            .iter()
            .zip(&incoming)
            .enumerate()
            .filter_map(|(index, (block, incoming))| {
                (Rc::strong_count(block) > *incoming).then_some(index)
            })
            .collect();
        while let Some(index) = pending.pop() {
            if !marked[index] {
                marked[index] = true;
                pending.extend(
                    blocks[index]
                        .borrow()
                        .iter()
                        .filter_map(Value::memory)
                        .map(|child| indices[&Rc::as_ptr(child)]),
                );
            }
        }
        for (block, reachable) in blocks.iter().zip(marked) {
            if !reachable {
                block.borrow_mut().clear();
            }
        }
        drop(blocks);
        self.blocks.retain(|block| block.strong_count() != 0);
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        // Release cycles even when execution ends before the next collection.
        let blocks: Vec<_> = self.blocks.iter().filter_map(Weak::upgrade).collect();
        for block in &blocks {
            block.borrow_mut().clear();
        }
    }
}
