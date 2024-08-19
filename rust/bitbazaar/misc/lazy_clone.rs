/// Efficient way to clone an item for each element in an iterator.
/// The final iteration will consume the original item, so no unnecessary clones are made.
pub trait IterWithCloneLazy {
    /// The return type of the iterator.
    type IterT;

    /// Efficient way to pass an owned clone of an item to each element in an iterator.
    /// Will pass the final item by value without cloning, so no unnecessary clones are made.
    fn with_clone_lazy<ItemT: Clone>(
        self,
        item: ItemT,
    ) -> impl Iterator<Item = (ItemT, Self::IterT)>
    where
        Self: Sized;
}

impl<IterT, I: IntoIterator<Item = IterT>> IterWithCloneLazy for I {
    type IterT = IterT;

    fn with_clone_lazy<ItemT: Clone>(
        self,
        item: ItemT,
    ) -> impl Iterator<Item = (ItemT, Self::IterT)>
    where
        Self: Sized,
    {
        let mut iter = self.into_iter();
        LazyCloneIter {
            item: Some(item),
            next_in_iter: iter.next(),
            iter,
        }
    }
}

struct LazyCloneIter<I: Iterator, ItemT: Clone> {
    // Will consume when next_in_iter is None, as on last iteration.
    item: Option<ItemT>,
    iter: I,
    next_in_iter: Option<I::Item>,
}

impl<I: Iterator, ItemT: Clone> Iterator for LazyCloneIter<I, ItemT> {
    type Item = (ItemT, I::Item);

    fn next(&mut self) -> Option<Self::Item> {
        self.next_in_iter.take().map(|next| {
            self.next_in_iter = self.iter.next();
            if self.next_in_iter.is_none() {
                (self.item.take().unwrap(), next)
            } else {
                (self.item.clone().unwrap(), next)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicUsize, Arc};

    use super::*;

    #[test]
    fn test_lazy_clone_with_clone_lazy() {
        struct Test {
            tot_clones: Arc<AtomicUsize>,
        }
        impl Clone for Test {
            fn clone(&self) -> Self {
                self.tot_clones
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Test {
                    tot_clones: self.tot_clones.clone(),
                }
            }
        }

        // Try for 0..10 iterator length, main things to check are 0, 1 and >1.
        // For all but final iteration, should clone, then pass by value.
        for count in 0..10 {
            let tot_clones = Arc::new(AtomicUsize::new(0));
            let test = Test {
                tot_clones: tot_clones.clone(),
            };
            for (t, index) in (0..count).with_clone_lazy(test) {
                assert_eq!(
                    t.tot_clones.load(std::sync::atomic::Ordering::Relaxed),
                    if index < count - 1 { index + 1 } else { index }
                );
            }
            assert_eq!(
                tot_clones.load(std::sync::atomic::Ordering::Relaxed),
                count.max(1) - 1
            );
        }
    }
}
