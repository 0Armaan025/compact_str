use std::iter::Extend;
use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};
use std::{
    alloc,
    fmt,
    mem,
    ptr,
    slice,
    str,
};

/// A soft limit on the amount of references that may be made to an `Arc`.
///
/// Going above this limit will abort your program (although not
/// necessarily) at _exactly_ `MAX_REFCOUNT + 1` references.
const MAX_REFCOUNT: usize = (isize::MAX) as usize;

#[repr(C)]
pub struct ArcString {
    len: usize,
    ptr: ptr::NonNull<ArcStringInner>,
}
unsafe impl Sync for ArcString {}
unsafe impl Send for ArcString {}

impl ArcString {
    #[inline]
    pub fn new(text: &str, additional: usize) -> Self {
        let len = text.len();
        let capacity = len + additional;
        let mut ptr = ArcStringInner::with_capacity(capacity);

        // SAFETY: We just created the `ArcStringInner` so we know the pointer is properly aligned,
        // it is non-null, points to an instance of `ArcStringInner`, and the `str_buffer`
        // is valid
        let buffer_ptr = unsafe { ptr.as_mut().str_buffer.as_mut_ptr() };
        // SAFETY: We know both `src` and `dest` are valid for respectively reads and writes of
        // length `len` because `len` comes from `src`, and `dest` was allocated to be at least that
        // length. We also know they're non-overlapping because `dest` is newly allocated
        unsafe { buffer_ptr.copy_from_nonoverlapping(text.as_ptr(), len) };

        ArcString { len, ptr }
    }

    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        debug_assert!(additional > 0);

        // Only reallocate if we don't have enough space for `additional` bytes
        if additional > self.capacity() - self.len() {
            let required = self.capacity() + additional;
            let amortized = 3 * self.capacity() / 2;
            let new_capacity = core::cmp::max(amortized, required);

            // TODO: Handle overflows in the case of __very__ large Strings
            debug_assert!(new_capacity > self.capacity());

            *self = ArcString::new(self.as_str(), new_capacity - self.len());
        }
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner().capacity
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: The only way you can construct an `ArcString` is via a `&str` so it must be valid
        // UTF-8, or the caller has manually made those guarantees
        unsafe { str::from_utf8_unchecked(self.as_slice()) }
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        &self.inner().as_bytes()[..self.len]
    }

    #[inline]
    pub fn push(&mut self, ch: char) {
        let len = self.len();
        let char_len = ch.len_utf8();

        // Make sure we have enough space for the char
        self.reserve(char_len);

        // SAFETY: We're writing valid UTF-8 into the slice
        let mut_slice = unsafe { self.make_mut_slice() };

        // We know we have enough space in the buffer, because we checked above
        ch.encode_utf8(&mut mut_slice[len..]);
        // Incrament the length of our string
        self.len += char_len;
    }

    #[inline]
    pub fn pop(&mut self) -> Option<char> {
        // Get the last character
        let c = self.as_str().chars().rev().next()?;
        // Decrement our length, effectively popping it
        self.len -= c.len_utf8();
        // Return the last character
        Some(c)
    }

    #[inline]
    pub fn push_str(&mut self, s: &str) {
        let len = self.len();
        let str_len = s.len();

        self.reserve(str_len);

        // SAFETY: We're writing valid UTF-8 into the slice
        let mut_slice = unsafe { self.make_mut_slice() };

        // We know we have enough space in the buffer, because we checked above
        mut_slice[len..len + str_len].copy_from_slice(s.as_bytes());
        // Incrament the length of our string
        self.len += str_len;
    }

    #[inline]
    pub unsafe fn make_mut_slice(&mut self) -> &mut [u8] {
        if self
            .inner()
            .ref_count
            .compare_exchange(1, 0, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // There is more than one reference to this underlying buffer, so we need to make a new
            // instance and decrement the count of the original by one

            // Make a new instance with the same capacity as self
            let additional = self.capacity() - self.len();
            let new = Self::new(self.as_str(), additional);

            // Assign self to our new instsance
            *self = new;
        } else {
            // We were the sole reference of either kind; bump back up the strong ref count.
            self.inner().ref_count.store(1, Ordering::Release);
        }

        // Return a mutable slice to the underlying buffer
        //
        // SAFETY: If we still have an instance of `ArcString` then we know the pointer to
        // `ArcStringInner` is valid for at least as long as the provided ref to `self`
        self.ptr.as_mut().as_mut_bytes()
    }

    #[inline]
    pub unsafe fn set_len(&mut self, length: usize) {
        self.len = length;
    }

    /// Returns a shared reference to the heap allocated `ArcStringInner`
    #[inline]
    fn inner(&self) -> &ArcStringInner {
        // SAFETY: If we still have an instance of `ArcString` then we know the pointer to
        // `ArcStringInner` is valid for at least as long as the provided ref to `self`
        unsafe { self.ptr.as_ref() }
    }

    #[inline(never)]
    unsafe fn drop_inner(&mut self) {
        ArcStringInner::dealloc(self.ptr)
    }
}

impl Clone for ArcString {
    fn clone(&self) -> Self {
        let old_count = self.inner().ref_count.fetch_add(1, Ordering::Relaxed);
        assert!(
            old_count < MAX_REFCOUNT,
            "Program has gone wild, ref count > {}",
            MAX_REFCOUNT
        );

        ArcString {
            len: self.len,
            ptr: self.ptr,
        }
    }
}

impl Drop for ArcString {
    fn drop(&mut self) {
        // This was copied from the implementation of `std::sync::Arc`
        // TODO: Better document the safety invariants here
        if self.inner().ref_count.fetch_sub(1, Ordering::Release) != 1 {
            return;
        }
        std::sync::atomic::fence(Ordering::Acquire);
        unsafe { self.drop_inner() }
    }
}

impl fmt::Debug for ArcString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl From<&str> for ArcString {
    fn from(text: &str) -> Self {
        ArcString::new(text, 0)
    }
}

impl Extend<char> for ArcString {
    fn extend<T: IntoIterator<Item = char>>(&mut self, iter: T) {
        let iterator = iter.into_iter();
        let (lower_bound, _) = iterator.size_hint();
        self.reserve(lower_bound);
        iterator.for_each(|c| self.push(c));
    }
}

impl<'a> Extend<&'a char> for ArcString {
    fn extend<T: IntoIterator<Item = &'a char>>(&mut self, iter: T) {
        self.extend(iter.into_iter().copied());
    }
}

impl<'a> Extend<&'a str> for ArcString {
    fn extend<T: IntoIterator<Item = &'a str>>(&mut self, iter: T) {
        iter.into_iter().for_each(|s| self.push_str(s));
    }
}

impl Extend<Box<str>> for ArcString {
    fn extend<T: IntoIterator<Item = Box<str>>>(&mut self, iter: T) {
        iter.into_iter().for_each(move |s| self.push_str(&s));
    }
}

impl Extend<String> for ArcString {
    fn extend<T: IntoIterator<Item = String>>(&mut self, iter: T) {
        iter.into_iter().for_each(move |s| self.push_str(&s));
    }
}

const UNKNOWN: usize = 0;
pub type StrBuffer = [u8; UNKNOWN];

#[repr(C)]
pub struct ArcStringInner {
    pub ref_count: AtomicUsize,
    capacity: usize,
    pub str_buffer: StrBuffer,
}

impl ArcStringInner {
    pub fn with_capacity(capacity: usize) -> ptr::NonNull<ArcStringInner> {
        let mut ptr = Self::alloc(capacity);

        // SAFETY: We just allocated an instance of `ArcStringInner` and checked to make sure it
        // wasn't null, so we know it's aligned properly, that it points to an instance of
        // `ArcStringInner` and that the "lifetime" is valid
        unsafe { ptr.as_mut().ref_count = AtomicUsize::new(1) };
        // SAFTEY: Same as above
        unsafe { ptr.as_mut().capacity = capacity };

        ptr
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: Since we have an instance of `ArcStringInner` so we know the buffer is still
        // valid, and we track the capacity with the creation and adjustment of the buffer
        unsafe { slice::from_raw_parts(self.str_buffer.as_ptr(), self.capacity) }
    }

    /// Returns a mutable reference to the underlying buffer of bytes
    ///
    /// # Invariants
    /// * The caller must assert that no other references, or instances of `ArcString` exist before
    /// calling this method. Otherwise multiple threads could race writing to the underlying buffer.
    /// * The caller must assert that the underlying buffer is still valid UTF-8
    #[inline]
    pub unsafe fn as_mut_bytes(&mut self) -> &mut [u8] {
        // SAFETY: Since we have an instance of `ArcStringInner` so we know the buffer is still
        // valid, and we track the capacity with the creation and adjustment of the buffer
        //
        // Note: In terms of mutability, it's up to the caller to assert the provided bytes are
        // value UTF-8
        slice::from_raw_parts_mut(self.str_buffer.as_mut_ptr(), self.capacity)
    }

    fn layout(capacity: usize) -> alloc::Layout {
        let buffer_layout = alloc::Layout::array::<u8>(capacity).unwrap();
        alloc::Layout::new::<Self>()
            .extend(buffer_layout)
            .unwrap()
            .0
            .pad_to_align()
    }

    pub fn alloc(capacity: usize) -> ptr::NonNull<ArcStringInner> {
        let layout = Self::layout(capacity);
        debug_assert!(layout.size() > 0);

        // SAFETY: `alloc(...)` has undefined behavior if the layout is zero-sized, but we know the
        // size of the layout is greater than 0 because we define it (and check for it above)
        let raw_ptr = unsafe { alloc::alloc(layout) as *mut ArcStringInner };

        // Check to make sure our pointer is non-null, some allocators return null pointers instead
        // of panicking
        match ptr::NonNull::new(raw_ptr) {
            Some(ptr) => ptr,
            None => alloc::handle_alloc_error(layout),
        }
    }

    pub fn dealloc(ptr: ptr::NonNull<ArcStringInner>) {
        // SAFETY: We know the pointer is non-null and it is properly aligned
        let capacity = unsafe { ptr.as_ref().capacity };
        let layout = Self::layout(capacity);

        // SAFETY: There is only one way to allocate an ArcStringInner, and it uses the same layout
        // we defined above. Also we know the pointer is non-null and we use the same global
        // allocator as we did in `Self::alloc(...)`
        unsafe { alloc::dealloc(ptr.as_ptr() as *mut u8, layout) };
    }
}

#[cfg(test)]
mod test {
    use proptest::prelude::*;
    use proptest::strategy::Strategy;

    use super::ArcString;

    #[test]
    fn test_empty() {
        let empty = "";
        let arc_str = ArcString::from(empty);

        assert_eq!(arc_str.as_str(), empty);
        assert_eq!(arc_str.len, empty.len());
    }

    #[test]
    fn test_long() {
        let long = "aaabbbcccdddeeefff\n
                    ggghhhiiijjjkkklll\n
                    mmmnnnooopppqqqrrr\n
                    ssstttuuuvvvwwwxxx\n
                    yyyzzz000111222333\n
                    444555666777888999000";
        let arc_str = ArcString::from(long);

        assert_eq!(arc_str.as_str(), long);
        assert_eq!(arc_str.len, long.len());
    }

    #[test]
    fn test_clone_and_drop() {
        let example = "hello world!";
        let arc_str_1 = ArcString::from(example);
        let arc_str_2 = arc_str_1.clone();

        drop(arc_str_1);

        assert_eq!(arc_str_2.as_str(), example);
        assert_eq!(arc_str_2.len, example.len());
    }

    #[test]
    fn test_sanity() {
        let example = "hello world!";
        let arc_str = ArcString::from(example);

        assert_eq!(arc_str.as_str(), example);
        assert_eq!(arc_str.len, example.len());
    }

    #[test]
    fn test_push() {
        let example = "hello world";
        let mut arc_str = ArcString::from(example);
        arc_str.push('!');

        assert_eq!(arc_str.as_str(), "hello world!");
        assert_eq!(arc_str.len(), 12);
    }

    #[test]
    fn test_pop() {
        let example = "hello";
        let mut arc_str = ArcString::from(example);

        assert_eq!(arc_str.pop(), Some('o'));
        assert_eq!(arc_str.pop(), Some('l'));
        assert_eq!(arc_str.pop(), Some('l'));

        assert_eq!(arc_str.as_str(), "he");
        assert_eq!(arc_str.len(), 2);
    }

    #[test]
    fn test_push_str() {
        let example = "hello";
        let mut arc_str = ArcString::from(example);

        arc_str.push_str(" world!");

        assert_eq!(arc_str.as_str(), "hello world!");
        assert_eq!(arc_str.len(), 12);
    }

    #[test]
    fn test_extend_chars() {
        let example = "hello";
        let mut arc_str = ArcString::from(example);

        arc_str.extend(" world!".chars());

        assert_eq!(arc_str.as_str(), "hello world!");
        assert_eq!(arc_str.len(), 12);
    }

    #[test]
    fn test_extend_strs() {
        let example = "hello";
        let mut arc_str = ArcString::from(example);

        let words = vec![" ", "world!", "my name is", " compact", "_str"];
        arc_str.extend(words);

        assert_eq!(arc_str.as_str(), "hello world!my name is compact_str");
        assert_eq!(arc_str.len(), 34);
    }

    // generates random unicode strings, upto 80 chars long
    fn rand_unicode() -> impl Strategy<Value = String> {
        proptest::collection::vec(proptest::char::any(), 0..80)
            .prop_map(|v| v.into_iter().collect())
    }

    proptest! {
        #[test]
        #[cfg_attr(miri, ignore)]
        fn test_strings_roundtrip(word in rand_unicode()) {
            let arc_str = ArcString::from(word.as_str());
            prop_assert_eq!(&word, arc_str.as_str());
        }
    }
}

static_assertions::const_assert_eq!(mem::size_of::<ArcString>(), 2 * mem::size_of::<usize>());
// Note: Although the compiler sees `ArcStringInner` as being 16 bytes, it's technically unsized
// because it contains a buffer of size `capacity`. We manually track the size of this buffer so
// `ArcString` can only be two words long
static_assertions::const_assert_eq!(
    mem::size_of::<ArcStringInner>(),
    2 * mem::size_of::<usize>()
);
