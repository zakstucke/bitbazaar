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

/// Binary search for the desired item in the array.
/// Returns None when not exactly found.
///
/// Arguments:
/// - arr_like: The array-like structure to search.
/// - comparer: `Fn(&Item)` -> "where to go from here".
/// - To support multiple items with the sort key, the `finalizer` will confirm the item is the one we want, checking all of those matching the sort key. If unique to the sort key just set to `|_| true`.
///
/// ARR SMALLEST TO LARGEST: `$target.cmp($item_val)`
/// ARR LARGEST TO SMALLEST: `$item_val.cmp(&$target)`
///
/// Returns:
/// - Some(index) the index of the item in the array.
/// - None if the item is not found or array empty.
pub fn binary_search_exact<Item>(
    arr_like: &impl BinarySearchable<Item = Item>,
    comparer: impl Fn(&Item) -> Ordering,
    finalizer: impl Fn(&Item) -> bool,
) -> Option<usize> {
    if arr_like.is_empty() {
        None
    } else {
        let mut low = 0;
        let mut high = arr_like.len() - 1;
        while low <= high {
            let mid = (low + high) / 2;
            match comparer(arr_like.get(mid)) {
                Ordering::Greater => low = mid + 1,
                Ordering::Less => high = if mid == 0 { return None } else { mid - 1 },
                Ordering::Equal => {
                    return finalize_index(
                        move |_index, item| comparer(item),
                        finalizer,
                        arr_like,
                        mid,
                    )
                }
            }
        }
        None
    }
}

/// Binary search for the desired item in the array, but also passes the prior and next items into the comparison function, if they exist.
/// Returns None when not exactly found.
///
/// Arguments:
/// - arr_like: The array-like structure to search.
/// - comparer: `Fn(Option<&Item>, &Item, Option<&Item>)` (prior, target, next) -> "where to go from here"
/// - To support multiple items with the sort key, the `finalizer` will confirm the item is the one we want, checking all of those matching the sort key. If unique to the sort key just set to `|_| true`.
///
/// ARR SMALLEST TO LARGEST: `$target.cmp($item_val)`
/// ARR LARGEST TO SMALLEST: `$item_val.cmp(&$target)`///
///
/// Returns:
/// - Some(index) the index of the item in the array.
/// - None if the item is not found or array empty.
pub fn binary_search_exact_with_siblings<Item>(
    arr_like: &impl BinarySearchable<Item = Item>,
    comparer: impl Fn(Option<&Item>, &Item, Option<&Item>) -> Ordering,
    finalizer: impl Fn(&Item) -> bool,
) -> Option<usize> {
    if arr_like.is_empty() {
        None
    } else {
        fn get_prior_and_next<Item>(
            arr_like: &impl BinarySearchable<Item = Item>,
            index: usize,
        ) -> (Option<&Item>, Option<&Item>) {
            let prior = if index == 0 {
                None
            } else {
                Some(arr_like.get(index - 1))
            };
            let next = if index == arr_like.len() - 1 {
                None
            } else {
                Some(arr_like.get(index + 1))
            };
            (prior, next)
        }

        let mut low = 0;
        let mut high = arr_like.len() - 1;
        while low <= high {
            let mid = (low + high) / 2;
            let (prior, next) = get_prior_and_next(arr_like, mid);
            match comparer(prior, arr_like.get(mid), next) {
                Ordering::Greater => low = mid + 1,
                Ordering::Less => high = if mid == 0 { return None } else { mid - 1 },
                Ordering::Equal => {
                    return finalize_index(
                        move |index, item| {
                            let (prior, next) = get_prior_and_next(arr_like, index);
                            comparer(prior, item, next)
                        },
                        finalizer,
                        arr_like,
                        mid,
                    )
                }
            }
        }
        None
    }
}

fn finalize_index<Item>(
    comparer: impl Fn(usize, &Item) -> Ordering,
    finalizer: impl Fn(&Item) -> bool,
    arr_like: &impl BinarySearchable<Item = Item>,
    index: usize,
) -> Option<usize> {
    // First check current index:
    if finalizer(arr_like.get(index)) {
        Some(index)
    } else {
        // Go back to the first index that still matches the sort key:
        let mut i = index;
        while i > 0 && matches!(comparer(i, arr_like.get(i - 1)), Ordering::Equal) {
            i -= 1;
            if finalizer(arr_like.get(i)) {
                return Some(i);
            }
        }
        // Go back up one if went too far:
        if !(matches!(comparer(i, arr_like.get(0)), Ordering::Equal)) {
            i += 1;
        }

        // Now go forward until finalizer is true, returning that index,
        // otherwise returning None if changes sort key or reaches end:
        while i < arr_like.len() && matches!(comparer(i, arr_like.get(i)), Ordering::Equal) {
            if finalizer(arr_like.get(i)) {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// Binary search for the desired item in the array.
/// Only returns None when the array is empty.
///
/// Arguments:
/// - arr_like: The array-like structure to search.
/// - comparer: `Fn(&Item)` -> "where to go from here"
///
/// ARR SMALLEST TO LARGEST: `$target.cmp($item_val)`
/// ARR LARGEST TO SMALLEST: `$item_val.cmp(&$target)`///
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
                Ordering::Less => high = if mid == 0 { return Some(0) } else { mid - 1 },
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
    fn test_binary_search_basic() {
        // Empty arrays shouldn't panic for any variation:
        assert_eq!(binary_search_exact(&[], |x| 5.cmp(x), |_| true), None);
        assert_eq!(
            binary_search_exact_with_siblings(&[], |_, x, _| 5.cmp(x), |_| true),
            None
        );
        assert_eq!(binary_search_soft(&[], |x| 5.cmp(x)), None);

        // 7 is missing, with exact should be None, with soft should be 5
        let arr = [1, 2, 3, 4, 5, 6, 8, 9, 10];
        assert_eq!(binary_search_exact(&arr, |x| 5.cmp(x), |_| true), Some(4));
        assert_eq!(binary_search_exact(&arr, |x| 1.cmp(x), |_| true), Some(0));
        assert_eq!(binary_search_exact(&arr, |x| 10.cmp(x), |_| true), Some(8));
        assert_eq!(binary_search_exact(&arr, |x| 7.cmp(x), |_| true), None);

        // With siblings just check they're being passed around correctly:
        assert_eq!(
            binary_search_exact_with_siblings(
                &arr,
                |prior, x, next| {
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
                },
                |_| true
            ),
            Some(4)
        );

        // Nonexact:
        assert_eq!(binary_search_soft(&arr, |x| 5.cmp(x)), Some(4));
        assert_eq!(binary_search_soft(&arr, |x| 1.cmp(x)), Some(0));
        assert_eq!(binary_search_soft(&arr, |x| 10.cmp(x)), Some(8));
        assert_eq!(binary_search_soft(&arr, |x| 7.cmp(x)), Some(5));
        assert_eq!(binary_search_soft(&[], |x| 7.cmp(x)), None);
    }

    /// Check no panic when item doesn't exist but should've been at the start (historic bug)
    #[test]
    fn test_binary_search_missing_from_start() {
        let arr = [2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(binary_search_exact(&arr, |x| 1.cmp(x), |_| true), None);
        assert_eq!(binary_search_soft(&arr, |x| 1.cmp(x)), Some(0));
        assert_eq!(
            binary_search_exact_with_siblings(&arr, |_, x, _| 1.cmp(x), |_| true),
            None
        );
    }

    /// Check no panic when item doesn't exist but should've been at the end (historic bug)
    #[test]
    fn test_binary_search_missing_from_end() {
        let arr = [1, 2, 3, 4, 5, 6, 7, 8, 9];
        assert_eq!(binary_search_exact(&arr, |x| 10.cmp(x), |_| true), None);
        assert_eq!(binary_search_soft(&arr, |x| 10.cmp(x)), Some(8));
        assert_eq!(
            binary_search_exact_with_siblings(&arr, |_, x, _| 10.cmp(x), |_| true),
            None
        );
    }

    /// - Confirm when duplicates with the same sort key, finalizer solves it.
    /// - Confirm when finalizer/array is dodgy and returns false for everything, just returns None.
    #[test]
    fn test_binary_search_finalizer() {
        struct Item {
            id: &'static str,
            sort_key: i32,
        }
        let arr = [
            Item {
                id: "b",
                sort_key: 2,
            },
            Item {
                id: "c",
                sort_key: 2,
            },
            Item {
                id: "d",
                sort_key: 2,
            },
            Item {
                id: "e",
                sort_key: 3,
            },
            Item {
                id: "f",
                sort_key: 4,
            },
            Item {
                id: "g",
                sort_key: 4,
            },
        ];

        assert_eq!(
            binary_search_exact(&arr, |x| 2.cmp(&x.sort_key), |x| x.id == "c"),
            Some(1)
        );
        assert_eq!(
            binary_search_exact_with_siblings(
                &arr,
                |_prior, x, _next| { 2.cmp(&x.sort_key) },
                |x| x.id == "c"
            ),
            Some(1)
        );

        assert_eq!(
            binary_search_exact(&arr, |x| 2.cmp(&x.sort_key), |x| x.id == "d"),
            Some(2)
        );
        assert_eq!(
            binary_search_exact_with_siblings(
                &arr,
                |_prior, x, _next| { 2.cmp(&x.sort_key) },
                |x| x.id == "d"
            ),
            Some(2)
        );

        assert_eq!(
            binary_search_exact(&arr, |x| 2.cmp(&x.sort_key), |x| x.id == "f"),
            None
        );
        assert_eq!(
            binary_search_exact_with_siblings(
                &arr,
                |_prior, x, _next| { 2.cmp(&x.sort_key) },
                |x| x.id == "f"
            ),
            None
        );
    }
}
