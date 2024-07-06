use std::cmp::Ordering;

/// A trait allowing arbitrary types to be binary searchable.
pub trait BinarySearchable {
    /// The type of the items in the array.
    type Item;

    /// Get the length of the array.
    fn len(&self) -> usize;

    /// Get the item at the given index.
    fn get(&self, index: usize) -> &Self::Item;

    /// Check if the array is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> BinarySearchable for Vec<T> {
    type Item = T;

    fn len(&self) -> usize {
        self.len()
    }

    fn get(&self, index: usize) -> &T {
        &self[index]
    }
}

impl<T, const N: usize> BinarySearchable for [T; N] {
    type Item = T;

    fn len(&self) -> usize {
        N
    }

    fn get(&self, index: usize) -> &T {
        &self[index]
    }
}

impl<T> BinarySearchable for [T] {
    type Item = T;

    fn len(&self) -> usize {
        self.len()
    }

    fn get(&self, index: usize) -> &T {
        &self[index]
    }
}

#[cfg(feature = "indexmap")]
impl<K, V> BinarySearchable for indexmap::IndexMap<K, V> {
    type Item = V;

    fn len(&self) -> usize {
        self.len()
    }

    fn get(&self, index: usize) -> &V {
        &self[index]
    }
}

/// Binary search for the desired item in the array.
/// Returns None when not exactly found.
///
/// Arguments:
/// - arr_like: The array-like structure to search.
/// - comparer: `Fn(&Item)` that should return `$target.cmp($item_val)`. (i.e. "where to go from here")
///
/// Returns:
/// - Some(index) the index of the item in the array.
/// - None if the item is not found.
pub fn binary_search_exact<Item>(
    arr_like: &impl BinarySearchable<Item = Item>,
    comparer: impl Fn(&Item) -> Ordering,
) -> Option<usize> {
    let mut low = 0;
    let mut high = arr_like.len() - 1;
    while low <= high {
        let mid = (low + high) / 2;
        match comparer(arr_like.get(mid)) {
            Ordering::Greater => low = mid + 1,
            Ordering::Less => high = mid - 1,
            Ordering::Equal => return Some(mid),
        }
    }
    None
}

/// Binary search for the desired item in the array, but also passes the prior and next items into the comparison function, if they exist.
/// Returns None when not exactly found.
///
/// Arguments:
/// - arr_like: The array-like structure to search.
/// - comparer: `Fn(Option<&Item>, &Item, Option<&Item>)` (prior, target, next) that should return `$target.cmp($item_val)`. (i.e. "where to go from here")
///
/// Returns:
/// - Some(index) the index of the item in the array.
/// - None if the item is not found.
pub fn binary_search_exact_with_siblings<Item>(
    arr_like: &impl BinarySearchable<Item = Item>,
    comparer: impl Fn(Option<&Item>, &Item, Option<&Item>) -> Ordering,
) -> Option<usize> {
    let mut low = 0;
    let mut high = arr_like.len() - 1;
    while low <= high {
        let mid = (low + high) / 2;
        let prior = if mid == 0 {
            None
        } else {
            Some(arr_like.get(mid - 1))
        };
        let next = if mid == arr_like.len() - 1 {
            None
        } else {
            Some(arr_like.get(mid + 1))
        };
        match comparer(prior, arr_like.get(mid), next) {
            Ordering::Greater => low = mid + 1,
            Ordering::Less => high = mid - 1,
            Ordering::Equal => return Some(mid),
        }
    }
    None
}

/// Binary search for the desired item in the array.
/// Only returns None when the array is empty.
///
/// Arguments:
/// - arr_like: The array-like structure to search.
/// - comparer: `Fn(&Item)` that should return `$target.cmp($item_val)`. (i.e. "where to go from here")
///
/// Returns:
/// - Some(index) the index of the item in the array, OR THE CLOSEST.
/// - None if the item is not found.
pub fn binary_search_soft<Item>(
    arr_like: &impl BinarySearchable<Item = Item>,
    comparer: impl Fn(&Item) -> Ordering,
) -> Option<usize> {
    if arr_like.is_empty() {
        None
    } else {
        let mut low = 0;
        let mut high = arr_like.len() - 1;
        let mut mid = 0;
        while low <= high {
            mid = (low + high) / 2;
            match comparer(arr_like.get(mid)) {
                Ordering::Greater => low = mid + 1,
                Ordering::Less => high = mid - 1,
                Ordering::Equal => return Some(mid),
            }
        }
        Some(mid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_search() {
        // 7 is missing, with exact should be None, with soft should be 5
        let arr = [1, 2, 3, 4, 5, 6, 8, 9, 10];
        assert_eq!(binary_search_exact(&arr, |x| 5.cmp(x)), Some(4));
        assert_eq!(binary_search_exact(&arr, |x| 1.cmp(x)), Some(0));
        assert_eq!(binary_search_exact(&arr, |x| 10.cmp(x)), Some(8));
        assert_eq!(binary_search_exact(&arr, |x| 7.cmp(x)), None);

        // With siblings just check they're being passed around correctly:
        assert_eq!(
            binary_search_exact_with_siblings(&arr, |prior, x, next| {
                if *x == 1 {
                    assert_eq!(prior, None);
                    assert_eq!(next, Some(&2));
                } else if *x == 10 {
                    assert_eq!(prior, Some(&9));
                    assert_eq!(next, None);
                } else if *x == 9 {
                    assert_eq!(prior, Some(&8));
                    assert_eq!(next, Some(&10));
                } else if *x == 8 {
                    assert_eq!(prior, Some(&6));
                    assert_eq!(next, Some(&9));
                } else {
                    // These are now all prior the gap so generic mapping:
                    assert_eq!(prior, Some(&arr[x - 2]));
                    assert_eq!(next, Some(&arr[*x]));
                }
                5.cmp(x)
            }),
            Some(4)
        );

        // Nonexact:
        assert_eq!(binary_search_soft(&arr, |x| 5.cmp(x)), Some(4));
        assert_eq!(binary_search_soft(&arr, |x| 1.cmp(x)), Some(0));
        assert_eq!(binary_search_soft(&arr, |x| 10.cmp(x)), Some(8));
        assert_eq!(binary_search_soft(&arr, |x| 7.cmp(x)), Some(5));
        assert_eq!(binary_search_soft(&[], |x| 7.cmp(x)), None);
    }
}
