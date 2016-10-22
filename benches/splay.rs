extern crate vec_arena;

use vec_arena::VecArena;

struct Node<T> {
    parent: usize,
    children: [usize; 2],
    value: T,
}

impl<T> Node<T> {
    fn new(value: T) -> Node<T> {
        Node {
            parent: !0,
            children: [!0, !0],
            value: value,
        }
    }
}

struct Splay<T> {
    arena: VecArena<Node<T>>,
    root: usize,
}

impl<T> Splay<T> where T: Ord + Eq + Clone {
    fn new() -> Splay<T> {
        Splay {
            arena: VecArena::new(),
            root: !0,
        }
    }

    fn rotate(&mut self, a: usize, b: usize) {
        let x = &mut self.arena;
        let p = x[a].parent;

        let dir = if x[a].children[0] == b { 0 } else { 1 };
        let t = x[b].children[dir ^ 1];

        x[a].children[dir] = t;
        if t != !0 {
            x[t].parent = a;
        }
        x[b].children[dir ^ 1] = a;
        x[a].parent = b;

        if p == !0 {
            self.root = b;
            x[b].parent = !0;
        } else {
            let dir = if x[p].children[0] == a { 0 } else { 1 };
            x[p].children[dir] = b;
            x[b].parent = p;
        }
    }

    fn splay(&mut self, a: usize) {
        loop {
            let b = self.arena[a].parent;
            if b == !0 {
                break;
            }

            let c = self.arena[b].parent;
            if c == !0 {
                self.rotate(b, a);
                break;
            }

            let is_l = self.arena[c].children[0] == b && self.arena[b].children[0] == a;
            let is_r = self.arena[c].children[1] == b && self.arena[b].children[1] == a;

            if is_l || is_r {
                self.rotate(c, b);
                self.rotate(b, a);
            } else {
                self.rotate(b, a);
                self.rotate(c, a);
            }
        }
    }

    fn insert(&mut self, value: T) {
        let node = self.arena.push(Node::new(value));

        if self.root == !0 {
            self.root = node;
        } else {
            let mut curr = self.root;
            loop {
                let dir = if self.arena[node].value < self.arena[curr].value { 0 } else { 1 };
                let next = self.arena[curr].children[dir];

                if next == !0 {
                    self.arena[curr].children[dir] = node;
                    self.arena[node].parent = curr;
                    self.splay(node);
                    break;
                } else {
                    curr = next;
                }
            }
        }
    }

    fn print(&self, node: usize, depth: usize) where T: std::fmt::Display {
        if node != !0 {
            self.print(self.arena[node].children[0], depth + 1);
            println!("{:width$}{}", "", self.arena[node].value, width = depth * 3);
            self.print(self.arena[node].children[1], depth + 1);
        }
    }
}

fn main() {
    let mut splay = Splay::new();

    let mut num = 1u32;
    for _ in 0..1000000 {
        num = num.wrapping_mul(17).wrapping_add(255);
        splay.insert(num);
    }
    // splay.print(splay.root, 0);
}
