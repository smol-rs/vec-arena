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

impl<T> Splay<T> where T: Ord {
    /// Constructs a new, empty splay tree.
    fn new() -> Splay<T> {
        Splay {
            arena: VecArena::new(),
            root: !0,
        }
    }

    /// Links nodes `p` and `c` as parent and child with the specified direction.
    #[inline(always)]
    fn link(&mut self, p: usize, c: usize, dir: usize) {
        self.arena[p].children[dir] = c;
        if c != !0 {
            self.arena[c].parent = p;
        }
    }

    /// Performs a rotation on node `c`, whose parent is node `p`.
    #[inline(always)]
    fn rotate(&mut self, p: usize, c: usize) {
        // Variables:
        // - `c` is the child node
        // - `p` is it's parent
        // - `g` is it's grandparent

        // Find the grandparent.
        let g = self.arena[p].parent;

        // The direction of p-c relationship.
        let dir = if self.arena[p].children[0] == c { 0 } else { 1 };

        // This is the child of `c` that needs to be reassigned to `p`.
        let t = self.arena[c].children[dir ^ 1];

        self.link(p, t, dir);
        self.link(c, p, dir ^ 1);

        if g == !0 {
            // There is no grandparent, so `c` becomes the root.
            self.root = c;
            self.arena[c].parent = !0;
        } else {
            // Link `g` and `c` together.
            let dir = if self.arena[g].children[0] == p { 0 } else { 1 };
            self.link(g, c, dir);
        }
    }

    /// Splays node
    fn splay(&mut self, c: usize) {
        loop {
            // Variables:
            // - `c` is the current node
            // - `p` is it's parent
            // - `g` is it's grandparent

            // Find the parent.
            let p = self.arena[c].parent;
            if p == !0 {
                // There is no parent. That means `c` is the root.
                break;
            }

            // Find the grandparent.
            let g = self.arena[p].parent;
            if g == !0 {
                // There is no grandparent. Just one rotation is left.
                // Zig step.
                self.rotate(p, c);
                break;
            }

            if (self.arena[g].children[0] == p) == (self.arena[p].children[0] == c) {
                // Zig-zig step.
                self.rotate(g, p);
                self.rotate(p, c);
            } else {
                // Zig-zag step.
                self.rotate(p, c);
                self.rotate(g, c);
            }
        }
    }

    /// Inserts a new node with specified `value`.
    fn insert(&mut self, value: T) {
        // Variables:
        // - `n` is the new node
        // - `p` will be it's parent
        // - `c` is the present child of `p`

        let n = self.arena.insert(Node::new(value));

        if self.root == !0 {
            self.root = n;
        } else {
            let mut p = self.root;
            loop {
                // Decide whether to go left or right.
                let dir = if self.arena[n].value < self.arena[p].value { 0 } else { 1 };
                let c = self.arena[p].children[dir];

                if c == !0 {
                    self.link(p, n, dir);
                    self.splay(n);
                    break;
                }
                p = c;
            }
        }
    }

    /// Pretty-prints the subtree rooted at `node`, indented by `depth` spaces.
    fn print(&self, node: usize, depth: usize) where T: std::fmt::Display {
        if node != !0 {
            // Print the left subtree.
            self.print(self.arena[node].children[0], depth + 1);

            // Print the current node.
            println!("{:width$}{}", "", self.arena[node].value, width = depth * 3);

            // Print the right subtree.
            self.print(self.arena[node].children[1], depth + 1);
        }
    }
}

fn main() {
    let mut splay = Splay::new();

    // Insert a bunch of pseudorandom numbers.
    let mut num = 1u32;
    for _ in 0..30 {
        num = num.wrapping_mul(17).wrapping_add(255);
        splay.insert(num);
    }

    // Display the whole splay tree.
    splay.print(splay.root, 0);
}
