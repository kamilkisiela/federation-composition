//! Insertion-ordered maps and sets with a fast deterministic hasher.
//!
//! Use these instead of `indexmap::IndexMap`/`IndexSet` (whose default hasher is SipHash with
//! a random seed) and instead of `std::collections::HashMap`/`HashSet` (whose iteration order
//! is unspecified, while JavaScript `Map`/`Set` iterate in insertion order). The hasher does
//! not affect iteration order: an `IndexMap` iterates in insertion order whatever it hashes
//! with. Construct them with `default()`, `with_capacity_and_hasher(n, Default::default())`,
//! `from_iter` or `collect()`; `new()`, `with_capacity()` and `from([..])` exist only for
//! the default hasher.

/// aHash with fixed seeds: the same hashes on every run and target, and no random seed (so no
/// `getrandom` in the wasm32 build).
#[derive(Clone, Copy, Debug, Default)]
pub struct AHashBuilder;

/// The seeds of [`AHashBuilder`] (the digits of pi, as any fixed values would do).
static SEEDS: ahash::RandomState = ahash::RandomState::with_seeds(
    0x243f_6a88_85a3_08d3,
    0x1319_8a2e_0370_7344,
    0xa409_3822_299f_31d0,
    0x082e_fa98_ec4e_6c89,
);

impl std::hash::BuildHasher for AHashBuilder {
    type Hasher = ahash::AHasher;

    fn build_hasher(&self) -> ahash::AHasher {
        SEEDS.build_hasher()
    }
}

/// An insertion-ordered map (`indexmap::IndexMap`) with [`AHashBuilder`].
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, AHashBuilder>;
/// An insertion-ordered set (`indexmap::IndexSet`) with [`AHashBuilder`].
pub type IndexSet<T> = indexmap::IndexSet<T, AHashBuilder>;
