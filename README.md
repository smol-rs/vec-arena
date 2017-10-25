# vec-arena (deprecated)

[![Build Status](https://travis-ci.org/stjepang/vec-arena.svg?branch=master)](https://travis-ci.org/stjepang/vec-arena)
[![License](https://img.shields.io/badge/license-Apache--2.0%2FMIT-blue.svg)](https://github.com/stjepang/vec-arena)
[![Cargo](https://img.shields.io/crates/v/vec-arena.svg)](https://crates.io/crates/vec-arena)
[![Documentation](https://docs.rs/vec-arena/badge.svg)](https://docs.rs/vec-arena)

**This crate is now deprecated in favor of [slab](https://github.com/carllerche/slab).**

#### What is this?

A simple object arena.

You want to build a doubly linked list? Or maybe a bidirectional tree? Perhaps an even more
complicated object graph?

Managing ownership and lifetimes might be tough then. Your options boil down to:

1. Use unsafe code to escape Rust's ownership rules.
2. Wrap every object in `Rc<RefCell<T>>`.
3. Use `Vec<T>` to store objects, then access them using indices.

If the last option seems most appealing to you, perhaps `Arena<T>` is for you.
It will provide a more convenient API than a plain `Vec<T>`.

#### Examples

Some data structures built using `Arena<T>`:

* [Doubly linked list](https://github.com/stjepang/vec-arena/blob/master/examples/linked_list.rs)
* [Splay tree](https://github.com/stjepang/vec-arena/blob/master/examples/splay_tree.rs)
