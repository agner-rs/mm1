#[derive(Debug, Clone)]
pub(super) struct Pool {
    root: Option<Box<Node>>,
}

impl Pool {
    pub(super) fn empty() -> Self {
        Self { root: None }
    }

    pub(super) fn new(addr: u64, mask: u64) -> Self {
        let mut pool = Self::empty();
        pool.release(addr, mask);
        pool
    }

    pub(super) fn release(&mut self, addr: u64, mask: u64) -> bool {
        util::assert_valid_mask(mask);
        debug_assert!(
            addr & !mask == 0,
            "`addr` contains no bits outside of the `mask` [addr: {:016x}/{}]",
            addr,
            mask.leading_ones()
        );

        fn dfs(cur_node_opt: &mut Option<Box<Node>>, addr: u64, mask: u64) -> bool {
            let Some(cur_node) = cur_node_opt else {
                *cur_node_opt = Some(Node::leaf(addr, mask).into());
                return true;
            };

            let cur_seg_path = cur_node.seg_path;
            let cur_seg_mask = cur_node.seg_mask;

            let (ca_addr, ca_mask) =
                util::common_ancestor((cur_seg_path, cur_seg_mask), (addr, mask));

            assert!(ca_mask.trailing_zeros() != 0);
            let split_bit = addr & (1u64 << (ca_mask.trailing_zeros() - 1));

            match (ca_mask == mask, ca_mask == cur_seg_mask) {
                (true, _) => {
                    // already in the pool
                    false
                },
                (false, true) => {
                    let Next::Fork { zero, one } = &mut cur_node.next else {
                        return false;
                    };

                    let inserted = dfs(if split_bit == 0 { zero } else { one }, addr, mask);

                    cur_node.maybe_compact();

                    inserted
                },
                (false, false) => {
                    // split
                    let sibling = std::mem::replace(cur_node, Node::leaf(ca_addr, ca_mask).into());
                    let this = Node::leaf(addr, mask);

                    let (zero, one) = if split_bit == 0 {
                        (Some(this.into()), Some(sibling))
                    } else {
                        (Some(sibling), Some(this.into()))
                    };
                    cur_node.next = Next::Fork { zero, one };
                    cur_node.maybe_compact();

                    true
                },
            }
        }

        let inserted = dfs(&mut self.root, addr, mask);

        self.check_invariants();
        inserted
    }

    pub(super) fn acquire(&mut self, mask: u64) -> Option<u64> {
        util::assert_valid_mask(mask);

        fn dfs(cur_node_opt: &mut Option<Box<Node>>, sought_mask: u64) -> Option<u64> {
            use std::cmp::Ordering::*;

            let cur_node = cur_node_opt.as_mut()?;

            let cur_seg_path = cur_node.seg_path;
            let cur_seg_mask = cur_node.seg_mask;

            let cur_leading_ones = cur_seg_mask.leading_ones();
            let sought_leading_ones = sought_mask.leading_ones();

            let (out, replace_with_opt) = match (
                cur_leading_ones.cmp(&sought_leading_ones),
                &mut cur_node.next,
            ) {
                (Greater, _) => return None,
                (Equal, Next::Fork { .. }) => return None,
                (Less, Next::Fork { zero, one }) => {
                    let (first_choice, second_choice) = (zero, one);
                    let out =
                        dfs(first_choice, sought_mask).or_else(|| dfs(second_choice, sought_mask));
                    let replace_with_opt = match (first_choice.is_some(), second_choice.is_some()) {
                        (true, true) => None,
                        (true, false) => Some(first_choice.take()),
                        (false, true) => Some(second_choice.take()),
                        (false, false) => panic!("how could that happen?"),
                    };
                    (out, replace_with_opt)
                },
                (Equal, Next::Leaf) => (Some(cur_seg_path), Some(None)),
                (Less, Next::Leaf) => {
                    let out = cur_seg_path;

                    let mut left_over = Some(Box::new(Node::leaf(
                        cur_seg_path | 1u64 << sought_mask.trailing_zeros(),
                        sought_mask,
                    )));

                    for i in (sought_mask.trailing_zeros() + 1)..cur_seg_mask.trailing_zeros() {
                        let split_bit = 1u64 << i;
                        let split_mask = u64::MAX << i;

                        let (zero, one) = (
                            left_over.take(),
                            Box::new(Node::leaf(cur_seg_path | split_bit, split_mask)).into(),
                        );

                        let parent = Node {
                            next: Next::Fork { zero, one },
                            ..Node::leaf(cur_seg_path, split_mask << 1)
                        };
                        left_over = Some(Box::new(parent));
                    }

                    (Some(out), Some(left_over))
                },
            };
            if let Some(replace_with) = replace_with_opt {
                let _ = std::mem::replace(cur_node_opt, replace_with);
            }
            out
        }

        let out = dfs(&mut self.root, mask);
        self.check_invariants();

        out
    }
}

mod util {
    pub(super) fn assert_valid_mask(mask: u64) {
        debug_assert_eq!(
            mask.leading_ones() + mask.trailing_zeros(),
            u64::BITS,
            "not a mask: {mask:064b}",
        );
    }

    pub(super) fn common_ancestor((la, lm): (u64, u64), (ra, rm): (u64, u64)) -> (u64, u64) {
        // eprintln!("---");
        assert_valid_mask(lm);
        assert_valid_mask(rm);
        debug_assert!(la & !lm == 0, "`la` contains no bits outside of the `lm`");
        debug_assert!(ra & !rm == 0, "`ra` contains no bits outside of the `rm`");

        // eprintln!("L:  {:064b}/{:064b}", la, lm);
        // eprintln!("R:  {:064b}/{:064b}", ra, rm);

        let m = lm & rm;
        // eprintln!("m:  {:64}/{:064b}", "", m);

        let cm = match (la & m ^ ra & m).leading_zeros() {
            0 => 0,
            c => u64::MAX << (u64::BITS - c),
        } & m;

        let ca = la & cm;
        debug_assert_eq!(ca, ra & cm);

        // eprintln!("->: {:064b}/{:064b}", cla, cm);

        assert_valid_mask(cm);
        debug_assert!(ca & !cm == 0, "`ca` contains no bits outside of the `cm`");

        (ca, cm)
    }
}

#[derive(Clone)]
struct Node {
    seg_path: u64,
    seg_mask: u64,
    next:     Next,
}

#[derive(Debug, Clone)]
enum Next {
    Leaf,
    Fork {
        zero: Option<Box<Node>>,
        one:  Option<Box<Node>>,
    },
}

impl Node {
    fn leaf(seg_path: u64, seg_mask: u64) -> Self {
        let node = Self {
            seg_path,
            seg_mask,
            next: Next::Leaf,
        };
        node.check_invariants();
        node
    }

    fn maybe_compact(&mut self) {
        let Next::Fork { zero, one } = &self.next else {
            return;
        };

        let zero = zero.as_ref().expect("should be set");
        let one = one.as_ref().expect("should be set");

        if zero.seg_mask << 1 == self.seg_mask
            && one.seg_mask << 1 == self.seg_mask
            && matches!(zero.next, Next::Leaf)
            && matches!(one.next, Next::Leaf)
        {
            self.next = Next::Leaf
        }
    }
}

mod invariants {
    use super::*;

    impl Pool {
        pub(super) fn check_invariants(&self) {
            #[cfg(debug_assertions)]
            {
                if let Some(root) = self.root.as_ref() {
                    root.check_invariants();
                }
            }
        }
    }

    impl Node {
        pub(super) fn check_invariants(&self) {
            #[cfg(debug_assertions)]
            {
                util::assert_valid_mask(self.seg_mask);
                assert_eq!(self.seg_path, self.seg_path & self.seg_mask);

                match self.seg_mask.trailing_zeros() {
                    0 => {
                        assert!(matches!(self.next, Next::Leaf));
                    },
                    nz => {
                        let min_leading_ones = u64::BITS - nz;
                        if let Next::Fork { zero, one } = &self.next {
                            assert!(zero.is_some());
                            assert!(one.is_some());
                            if let Some(zero) = zero.as_ref() {
                                assert!(min_leading_ones <= zero.seg_mask.leading_ones());
                                zero.check_invariants();
                            }
                            if let Some(one) = one.as_ref() {
                                assert!(min_leading_ones <= one.seg_mask.leading_ones());
                                one.check_invariants();
                            }
                        }
                    },
                }
            }
        }
    }
}

mod fmt {
    use std::fmt;

    use super::Node;

    impl fmt::Debug for Node {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Node")
                .field(
                    "addr",
                    &format!("{:016x} ({:064b})", self.seg_path, self.seg_path),
                )
                .field(
                    "mask",
                    &format!("{:016x} ({:064b})", self.seg_mask, self.seg_mask),
                )
                .field("next", &self.next)
                .finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Pool, util};

    #[test]
    fn leading_and_trailing_zeroes_and_ones() {
        assert_eq!(u64::MIN.leading_ones(), 0);
        assert_eq!(u64::MIN.leading_zeros(), u64::BITS);
        assert_eq!(u64::MIN.trailing_ones(), 0);
        assert_eq!(u64::MIN.trailing_zeros(), u64::BITS);

        assert_eq!(u64::MAX.leading_ones(), u64::BITS);
        assert_eq!(u64::MAX.leading_zeros(), 0);
        assert_eq!(u64::MAX.trailing_ones(), u64::BITS);
        assert_eq!(u64::MAX.trailing_zeros(), 0);
    }

    #[test]
    fn common_ancestor() {
        assert_eq!(
            util::common_ancestor(
                (0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF),
                (0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF),
            ),
            (0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF)
        );

        assert_eq!(
            util::common_ancestor(
                (0xFFFF_FFFF_FFFF_FFF0 | 0b1111, 0xFFFF_FFFF_FFFF_FFFF),
                (0xFFFF_FFFF_FFFF_FFF0 | 0b0111, 0xFFFF_FFFF_FFFF_FFFF),
            ),
            (0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0)
        );

        assert_eq!(
            util::common_ancestor(
                (0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0),
                (0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFFF),
            ),
            (0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0)
        );

        assert_eq!(
            util::common_ancestor(
                (0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0),
                (0xFFFF_FFFF_FFFF_FF00, 0xFFFF_FFFF_FFFF_FF00),
            ),
            (0xFFFF_FFFF_FFFF_FF00, 0xFFFF_FFFF_FFFF_FF00)
        );
        assert_eq!(
            util::common_ancestor(
                (0xFFFF_FFFF_FFF0_FFF0, 0xFFFF_FFFF_FFFF_FFF0),
                (0xFFFF_FFFF_FFFF_FF00, 0xFFFF_FFFF_FFFF_FF00),
            ),
            (0xFFFF_FFFF_FFF0_0000, 0xFFFF_FFFF_FFF0_0000)
        );
        assert_eq!(
            util::common_ancestor(
                (0x0FFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF),
                (0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF),
            ),
            (0x0000_0000_0000_0000, 0x0000_0000_0000_0000)
        );
    }

    #[test]
    fn pool_01() {
        let mut pool = Pool::empty();
        assert!(pool.release(0xFFFF_FFFF_FFFF_FFFE, 0xFFFF_FFFF_FFFF_FFFF));
        assert!(pool.release(0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF));

        insta::assert_debug_snapshot!(&pool);
    }

    #[test]
    fn pool_02() {
        let mut pool = Pool::empty();
        assert!(pool.release(0xFFFF_FFFF_FFFF_FF00, 0xFFFF_FFFF_FFFF_FF00));
        assert!(!pool.release(0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0));
        insta::assert_debug_snapshot!(&pool);
    }

    #[test]
    fn pool_03() {
        let mut pool = Pool::empty();
        assert!(pool.release(0xFFFF_FFFF_FFFF_FF00, 0xFFFF_FFFF_FFFF_FF00));
        let addr = pool.acquire(0xFFFF_FFFF_FFFF_FF00).expect("/56");
        insta::assert_debug_snapshot!((addr, &pool));
    }

    #[test]
    fn pool_04() {
        let mut pool = Pool::empty();
        assert!(pool.release(0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0));
        let a_61 = pool.acquire(0xFFFF_FFFF_FFFF_FFF0 | 0b1000).expect("/61");
        let a_62 = pool.acquire(0xFFFF_FFFF_FFFF_FFF0 | 0b1100).expect("/62");
        let a_63 = pool.acquire(0xFFFF_FFFF_FFFF_FFF0 | 0b1110).expect("/63");
        let a_64 = pool.acquire(0xFFFF_FFFF_FFFF_FFF0 | 0b1111).expect("/64");
        insta::assert_debug_snapshot!(([a_61, a_62, a_63, a_64,], &pool));
    }

    #[test]
    fn pool_05() {
        let mut pool = Pool::empty();
        assert!(pool.release(0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF));
        let a_64 = pool.acquire(0xFFFF_FFFF_FFFF_FFFF).expect("/64");
        insta::assert_debug_snapshot!((a_64, &pool));
    }

    #[test]
    fn pool_06() {
        let mut pool = Pool::new(0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0);
        let mut addresses = vec![];
        while let Some(a) = pool.acquire(0xFFFF_FFFF_FFFF_FFFF) {
            eprintln!("- {a:016x}");
            // eprintln!("   {:#?}", pool);
            addresses.push(a);
        }
        assert_eq!(addresses.len(), 16);
        for a in addresses {
            assert!(pool.release(a, 0xFFFF_FFFF_FFFF_FFFF));
        }

        insta::assert_debug_snapshot!(&pool);
    }

    #[test]
    fn pool_07() {
        let mut pool = Pool::new(0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0);
        let mut addresses = vec![];
        while let Some(a) = pool.acquire(0xFFFF_FFFF_FFFF_FFFF) {
            eprintln!("- {a:016x}");
            // eprintln!("   {:#?}", pool);
            addresses.push(a);
        }
        addresses.sort_by_key(|a| a.reverse_bits());
        eprintln!("{addresses:#?}");
        assert_eq!(addresses.len(), 16);
        for a in addresses {
            assert!(pool.release(a, 0xFFFF_FFFF_FFFF_FFFF));
        }

        insta::assert_debug_snapshot!(&pool);
    }

    #[test]
    fn pool_08() {
        let mut pool_a = Pool::new(0xFFFF_FFFF_FFFF_FFF0, 0xFFFF_FFFF_FFFF_FFF0);
        let mut pool_b = Pool::empty();

        let mut acquired = vec![];
        while let Some(a) = pool_a.acquire(0xFFFF_FFFF_FFFF_FFFF) {
            eprintln!("- {a:016x}");
            acquired.push(a);
        }

        while let Some(a) = acquired.pop() {
            pool_b.release(a, 0xFFFF_FFFF_FFFF_FFFF);
        }
    }

    #[test]
    fn pool_09() {
        let mut pool_a = Pool::new(0xFFFF_FFFF_FFFF_FF00, 0xFFFF_FFFF_FFFF_FF00);
        let mut pool_b = Pool::empty();

        let mut acquired = vec![];
        while let Some(a) = pool_a.acquire(0xFFFF_FFFF_FFFF_FFFF) {
            eprintln!("- {a:016x}");
            acquired.push(a);
        }

        acquired.sort_by_cached_key(|a| {
            use std::hash::Hasher;
            let mut h = std::hash::DefaultHasher::new();
            h.write_u64(*a);
            h.finish()
        });

        while let Some(a) = acquired.pop() {
            pool_b.release(a, 0xFFFF_FFFF_FFFF_FFFF);
        }
    }
}
