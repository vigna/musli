use core::borrow::Borrow;
use core::fmt;
use core::marker::PhantomData;

use crate::de::{Decoder, TypeHint};
use crate::expecting::{self, Expecting};
use crate::no_std::ToOwned;
use crate::Context;

/// A visitor for data where it might be possible to borrow it without copying
/// from the underlying [Decoder].
///
/// A visitor is required with [Decoder::decode_bytes] and
/// [Decoder::decode_string] because the caller doesn't know if the encoding
/// format is capable of producing references to the underlying data directly or
/// if it needs to be processed.
///
/// By requiring a visitor we ensure that the caller has to handle both
/// scenarios, even if one involves erroring. A type like
/// [Cow][std::borrow::Cow] is an example of a type which can comfortably handle
/// both.
///
/// [Decoder]: crate::de::Decoder
/// [Decoder::decode_bytes]: crate::de::Decoder::decode_bytes
/// [Decoder::decode_string]: crate::de::Decoder::decode_string
pub trait ValueVisitor<'de, C: ?Sized + Context, T>: Sized
where
    T: ?Sized + ToOwned,
{
    /// The value produced.
    type Ok;

    /// Format an error indicating what was expected by this visitor.
    ///
    /// Override to be more specific about the type that failed.
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    /// Visit an owned value.
    #[inline]
    fn visit_owned(self, cx: &C, value: T::Owned) -> Result<Self::Ok, C::Error> {
        self.visit_ref(cx, value.borrow())
    }

    /// Visit a string that is borrowed directly from the source data.
    #[inline]
    fn visit_borrowed(self, cx: &C, value: &'de T) -> Result<Self::Ok, C::Error> {
        self.visit_ref(cx, value)
    }

    /// Visit a value reference that is provided from the decoder in any manner
    /// possible. Which might require additional decoding work.
    #[inline]
    fn visit_ref(self, cx: &C, _: &T) -> Result<Self::Ok, C::Error> {
        Err(cx.message(expecting::bad_visitor_type(
            &expecting::AnyValue,
            ExpectingWrapper::new(&self),
        )))
    }

    /// Fallback used when the type is either not implemented for this visitor
    /// or the underlying format doesn't know which type to decode.
    #[inline]
    fn visit_any<D>(self, cx: &C, _: D, hint: TypeHint) -> Result<Self::Ok, C::Error>
    where
        D: Decoder<'de, C>,
    {
        Err(cx.message(expecting::unsupported_type(
            &hint,
            ExpectingWrapper::new(&self),
        )))
    }
}

#[repr(transparent)]
struct ExpectingWrapper<'a, T, C: ?Sized, I: ?Sized> {
    inner: T,
    _marker: PhantomData<(&'a C, &'a I)>,
}

impl<'a, T, C: ?Sized, U: ?Sized> ExpectingWrapper<'a, T, C, U> {
    #[inline]
    fn new(value: &T) -> &Self {
        // SAFETY: `ExpectingWrapper` is repr(transparent) over `T`.
        unsafe { &*(value as *const T as *const Self) }
    }
}

impl<'a, 'de, T, C, U> Expecting for ExpectingWrapper<'a, T, C, U>
where
    T: ValueVisitor<'de, C, U>,
    C: ?Sized + Context,
    U: ?Sized + ToOwned,
{
    #[inline]
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(f)
    }
}
