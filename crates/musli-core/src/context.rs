//! Things related to working with contexts.

use core::error::Error;
use core::fmt;
use core::str;

use crate::Allocator;

/// Provides ergonomic access to the serialization context.
///
/// This is used to among other things report diagnostics.
pub trait Context: Copy {
    /// Error produced by the context.
    type Error;
    /// A mark during processing.
    type Mark;
    /// The allocator associated with the context.
    type Allocator: Allocator;

    /// Clear the state of the context, allowing it to be re-used.
    fn clear(self);

    /// Advance the context by `n` bytes of input.
    ///
    /// This is typically used to move the mark forward as produced by
    /// [Context::mark].
    fn advance(self, n: usize);

    /// Return a mark which acts as a checkpoint at the current encoding state.
    ///
    /// The context is in a privileged state in that it sees everything, so a
    /// mark can be quite useful for determining the context of an error.
    ///
    /// This typically indicates a byte offset, and is used by
    /// [`message_at`][Context::message_at] to report a spanned error.
    fn mark(self) -> Self::Mark;

    /// Restore the state of the context to the specified mark.
    fn restore(self, mark: &Self::Mark);

    /// Access the underlying allocator.
    fn alloc(self) -> Self::Allocator;

    /// Generate a map function which maps an error using the `custom` function.
    #[inline]
    fn map<E>(self) -> impl FnOnce(E) -> Self::Error
    where
        E: 'static + Send + Sync + Error,
    {
        move |error| self.custom(error)
    }

    /// Report a custom error, which is not encapsulated by the error type
    /// expected by the context. This is essentially a type-erased way of
    /// reporting error-like things out from the context.
    fn custom<E>(self, error: E) -> Self::Error
    where
        E: 'static + Send + Sync + Error;

    /// Report a message as an error.
    ///
    /// This is made available to format custom error messages in `no_std`
    /// environments. The error message is to be collected by formatting `T`.
    fn message<M>(self, message: M) -> Self::Error
    where
        M: fmt::Display;

    /// Report an error based on a mark.
    ///
    /// A mark is generated using [Context::mark] and indicates a prior state.
    #[inline]
    fn message_at<M>(self, mark: &Self::Mark, message: M) -> Self::Error
    where
        M: fmt::Display,
    {
        _ = mark;
        self.message(message)
    }

    /// Report an error based on a mark.
    ///
    /// A mark is generated using [Context::mark] and indicates a prior state.
    #[inline]
    fn custom_at<E>(self, mark: &Self::Mark, message: E) -> Self::Error
    where
        E: 'static + Send + Sync + Error,
    {
        _ = mark;
        self.custom(message)
    }

    /// Indicate that we've entered a struct with the given `name`.
    ///
    /// The `name` variable corresponds to the identifiers of the struct.
    ///
    /// This will be matched with a corresponding call to [`leave_struct`].
    ///
    /// [`leave_struct`]: Context::leave_struct
    #[inline]
    fn enter_struct(self, type_name: &'static str) {
        _ = type_name;
    }

    /// Trace that we've left the last struct that was entered.
    #[inline]
    fn leave_struct(self) {}

    /// Indicate that we've entered an enum with the given `name`.
    ///
    /// The `name` variable corresponds to the identifiers of the enum.
    ///
    /// This will be matched with a corresponding call to [`leave_enum`].
    ///
    /// [`leave_enum`]: Context::leave_enum
    #[inline]
    fn enter_enum(self, type_name: &'static str) {
        _ = type_name;
    }

    /// Trace that we've left the last enum that was entered.
    #[inline]
    fn leave_enum(self) {}

    /// Trace that we've entered the given named field.
    ///
    /// A named field is part of a regular struct, where the literal field name
    /// is the `name` argument below, and the musli tag being used for the field
    /// is the second argument.
    ///
    /// This will be matched with a corresponding call to [`leave_field`].
    ///
    /// Here `name` is `"field"` and `tag` is `"string"`.
    ///
    /// ```
    /// use musli::{Decode, Encode};
    ///
    /// #[derive(Decode, Encode)]
    /// #[musli(name_all = "name")]
    /// struct Struct {
    ///     #[musli(name = "string")]
    ///     field: String,
    /// }
    /// ```
    ///
    /// [`leave_field`]: Context::leave_field
    #[inline]
    fn enter_named_field<F>(self, type_name: &'static str, field: F)
    where
        F: fmt::Display,
    {
        _ = type_name;
        _ = field;
    }

    /// Trace that we've entered the given unnamed field.
    ///
    /// An unnamed field is part of a tuple struct, where the field index is the
    /// `index` argument below, and the musli tag being used for the field is
    /// the second argument.
    ///
    /// This will be matched with a corresponding call to [`leave_field`].
    ///
    /// Here `index` is `0` and `name` is `"string"`.
    ///
    /// ```
    /// use musli::{Decode, Encode};
    ///
    /// #[derive(Decode, Encode)]
    /// #[musli(name_all = "name")]
    /// struct Struct(#[musli(name = "string")] String);
    /// ```
    ///
    /// [`leave_field`]: Context::leave_field
    #[inline]
    fn enter_unnamed_field<F>(self, index: u32, name: F)
    where
        F: fmt::Display,
    {
        _ = index;
        _ = name;
    }

    /// Trace that we've left the last field that was entered.
    ///
    /// The `marker` argument will be the same as the one returned from
    /// [`enter_named_field`] or [`enter_unnamed_field`].
    ///
    /// [`enter_named_field`]: Context::enter_named_field
    /// [`enter_unnamed_field`]: Context::enter_unnamed_field
    #[inline]
    fn leave_field(self) {}

    /// Trace that we've entered the given variant in an enum.
    ///
    /// A named variant is part of an enum, where the literal variant name is
    /// the `name` argument below, and the musli tag being used to decode the
    /// variant is the second argument.
    ///
    /// This will be matched with a corresponding call to
    /// [`leave_variant`] with the same marker provided as an argument as
    /// the one returned here.
    ///
    /// Here `name` is `"field"` and `tag` is `"string"`.
    ///
    /// ```
    /// use musli::{Decode, Encode};
    ///
    /// #[derive(Decode, Encode)]
    /// #[musli(name_all = "name")]
    /// struct Struct {
    ///     #[musli(name = "string")]
    ///     field: String,
    /// }
    /// ```
    ///
    /// [`leave_variant`]: Context::leave_variant
    #[inline]
    fn enter_variant<V>(self, type_name: &'static str, tag: V)
    where
        V: fmt::Display,
    {
        _ = type_name;
        _ = tag;
    }

    /// Trace that we've left the last variant that was entered.
    ///
    /// The `marker` argument will be the same as the one returned from
    /// [`enter_variant`].
    ///
    /// [`enter_variant`]: Context::enter_variant
    #[inline]
    fn leave_variant(self) {}

    /// Trace a that a map key has been entered.
    #[inline]
    fn enter_map_key<K>(self, field: K)
    where
        K: fmt::Display,
    {
        _ = field;
    }

    /// Trace that we've left the last map field that was entered.
    ///
    /// The `marker` argument will be the same as the one returned from
    /// [`enter_map_key`].
    ///
    /// [`enter_map_key`]: Context::enter_map_key
    #[inline]
    fn leave_map_key(self) {}

    /// Trace a sequence field.
    #[inline]
    fn enter_sequence_index(self, index: usize) {
        _ = index;
    }

    /// Trace that we've left the last sequence index that was entered.
    ///
    /// The `marker` argument will be the same as the one returned from
    /// [`enter_sequence_index`].
    ///
    /// [`enter_sequence_index`]: Context::enter_sequence_index
    #[inline]
    fn leave_sequence_index(self) {}
}
