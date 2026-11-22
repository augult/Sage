THOUGHT = r'''The skeleton fixes the public API: `Matcher::new(matcher, subs)`, `Fizzy::new()`, `Fizzy::add_matcher(self, Matcher<T>) -> Self`, `Fizzy::apply(iter) -> impl Iterator<Item = String>`, and a generic `fizz_buzz<T>()`.

Design:
- `Matcher<T>` stores a boxed `Fn(T) -> bool` and the substitution word as `String`, with `S: ToString` so both `&str` and `String` work.
- `Fizzy<T>` stores a `Vec<Matcher<T>>`. `apply` maps each item by concatenating the substitution of every matcher that fires, falling back to the item's own display form when none do. Items are consumed once per matcher, so `T: Copy` and `T: Display`.
- `fizz_buzz<T>()` needs modular arithmetic generically, so it requires `T: Copy + Display + PartialEq + From<u8>` and `Rem<Output = T>`, comparing remainders against `T::from(0u8)`.

Concat order matters: adding the 3 rule before the 5 rule yields "fizzbuzz".'''
CONTENT = r'''Implementing the matcher and Fizzy types, plus the standard rules factory.'''
CALLS = [
    {"name": "write", "arguments": {"filePath": "src/lib.rs", "content": r'''use std::fmt::Display;
use std::ops::Rem;

/// A Matcher is a single rule of fizzbuzz: given a function on T, should
/// a word be substituted in? If yes, which word?
pub struct Matcher<T> {
    matcher: Box<dyn Fn(T) -> bool>,
    subs: String,
}

impl<T> Matcher<T> {
    pub fn new<F, S>(matcher: F, subs: S) -> Matcher<T>
    where
        F: Fn(T) -> bool + 'static,
        S: ToString,
    {
        Matcher {
            matcher: Box::new(matcher),
            subs: subs.to_string(),
        }
    }
}

/// A Fizzy is a set of matchers, which may be applied to an iterator.
pub struct Fizzy<T> {
    matchers: Vec<Matcher<T>>,
}

impl<T> Fizzy<T> {
    pub fn new() -> Self {
        Fizzy {
            matchers: Vec::new(),
        }
    }

    #[must_use]
    pub fn add_matcher(mut self, matcher: Matcher<T>) -> Self {
        self.matchers.push(matcher);
        self
    }

    /// map this fizzy onto every element of an iterator, returning a new iterator
    pub fn apply<I>(self, iter: I) -> impl Iterator<Item = String>
    where
        I: Iterator<Item = T>,
        T: Copy + Display,
    {
        iter.map(move |item| {
            let substituted: String = self
                .matchers
                .iter()
                .filter(|matcher| (matcher.matcher)(item))
                .map(|matcher| matcher.subs.clone())
                .collect();
            if substituted.is_empty() {
                item.to_string()
            } else {
                substituted
            }
        })
    }
}

impl<T> Default for Fizzy<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// convenience function: return a Fizzy which applies the standard fizz-buzz rules
pub fn fizz_buzz<T>() -> Fizzy<T>
where
    T: Copy + Display + From<u8> + PartialEq + Rem<Output = T>,
{
    let zero = T::from(0u8);
    Fizzy::new()
        .add_matcher(Matcher::new(move |n: T| n % T::from(3u8) == zero, "fizz"))
        .add_matcher(Matcher::new(move |n: T| n % T::from(5u8) == zero, "buzz"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_fizz_buzz() {
        let actual: Vec<String> = fizz_buzz::<i32>().apply(1..=16).collect();
        let expected = [
            "1", "2", "fizz", "4", "buzz", "fizz", "7", "8", "fizz", "buzz", "11", "fizz", "13",
            "14", "fizzbuzz", "16",
        ];
        assert_eq!(actual, expected);
    }

    #[test]
    fn works_on_other_numeric_types() {
        let actual: Vec<String> = fizz_buzz::<u64>().apply(1..=5).collect();
        assert_eq!(actual, ["1", "2", "fizz", "4", "buzz"]);
    }

    #[test]
    fn custom_matchers() {
        let actual: Vec<String> = Fizzy::new()
            .add_matcher(Matcher::new(|n: i32| n % 2 == 0, "even"))
            .add_matcher(Matcher::new(|n: i32| n % 3 == 0, "three"))
            .apply(1..=6)
            .collect();
        assert_eq!(actual, ["1", "even", "three", "even", "5", "eventhree"]);
    }

    #[test]
    fn works_on_an_infinite_iterator() {
        let actual: Vec<String> = Fizzy::new()
            .add_matcher(Matcher::new(|n: i64| n == 1, "one"))
            .apply(1..)
            .take(3)
            .collect();
        assert_eq!(actual, ["one", "2", "3"]);
    }
}
'''}},
    {"name": "bash", "arguments": {"command": "cargo test --quiet 2>&1 | tail -30"}},
]
