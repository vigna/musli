use core::fmt;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use musli::de::{
    Decoder, MapDecoder, MapEntriesDecoder, MapEntryDecoder, PackDecoder, SequenceDecoder,
    SizeHint, StructDecoder, StructFieldDecoder, StructFieldsDecoder, ValueVisitor, VariantDecoder,
};
use musli::{Context, Decode};

use crate::options::Options;
use crate::reader::Reader;

/// A very simple decoder suitable for storage decoding.
pub struct StorageDecoder<'a, R, const F: Options, C: ?Sized> {
    cx: &'a C,
    reader: R,
}

impl<'a, R, const F: Options, C: ?Sized> StorageDecoder<'a, R, F, C> {
    /// Construct a new fixed width message encoder.
    #[inline]
    pub fn new(cx: &'a C, reader: R) -> Self {
        Self { cx, reader }
    }
}

/// A length-prefixed decode wrapper.
///
/// This simplifies implementing decoders that do not have any special handling
/// for length-prefixed types.
#[doc(hidden)]
pub struct LimitedStorageDecoder<'a, R, const F: Options, C: ?Sized> {
    remaining: usize,
    decoder: StorageDecoder<'a, R, F, C>,
}

#[musli::decoder]
impl<'a, 'de, R, const F: Options, C: ?Sized + Context> Decoder<'de> for StorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type Error = C::Error;
    type Mode = C::Mode;
    type WithContext<'this, U> = StorageDecoder<'this, R, F, U> where U: 'this + Context;
    type DecodePack = Self;
    type DecodeSome = Self;
    type DecodeSequence = LimitedStorageDecoder<'a, R, F, C>;
    type DecodeTuple = Self;
    type DecodeMap = LimitedStorageDecoder<'a, R, F, C>;
    type DecodeStruct = LimitedStorageDecoder<'a, R, F, C>;
    type DecodeVariant = Self;

    fn cx(&self) -> &C {
        self.cx
    }

    #[inline]
    fn with_context<U>(self, cx: &U) -> Result<Self::WithContext<'_, U>, C::Error>
    where
        U: Context,
    {
        Ok(StorageDecoder::new(cx, self.reader))
    }

    #[inline]
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "type supported by the storage decoder")
    }

    #[inline]
    fn decode<T>(self) -> Result<T, Self::Error>
    where
        T: Decode<'de, Self::Mode>,
    {
        self.cx.decode(self)
    }

    #[inline]
    fn decode_unit(mut self) -> Result<(), C::Error> {
        let mark = self.cx.mark();
        let count = musli_common::int::decode_usize::<_, _, F>(self.cx, self.reader.borrow_mut())?;

        if count != 0 {
            return Err(self
                .cx
                .marked_message(mark, ExpectedEmptySequence { actual: count }));
        }

        Ok(())
    }

    #[inline]
    fn decode_pack(self) -> Result<Self::DecodePack, C::Error> {
        Ok(self)
    }

    #[inline]
    fn decode_array<const N: usize>(mut self) -> Result<[u8; N], C::Error> {
        self.reader.read_array(self.cx)
    }

    #[inline]
    fn decode_bytes<V>(mut self, visitor: V) -> Result<V::Ok, C::Error>
    where
        V: ValueVisitor<'de, C, [u8]>,
    {
        let len = musli_common::int::decode_usize::<_, _, F>(self.cx, self.reader.borrow_mut())?;
        self.reader.read_bytes(self.cx, len, visitor)
    }

    #[inline]
    fn decode_string<V>(self, visitor: V) -> Result<V::Ok, C::Error>
    where
        V: ValueVisitor<'de, C, str>,
    {
        struct Visitor<V>(V);

        impl<'de, C, V> ValueVisitor<'de, C, [u8]> for Visitor<V>
        where
            C: ?Sized + Context,
            V: ValueVisitor<'de, C, str>,
        {
            type Ok = V::Ok;

            #[inline]
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.expecting(f)
            }

            #[cfg(feature = "alloc")]
            #[inline]
            fn visit_owned(self, cx: &C, bytes: Vec<u8>) -> Result<Self::Ok, C::Error> {
                let string = musli_common::str::from_utf8_owned(bytes).map_err(cx.map())?;
                self.0.visit_owned(cx, string)
            }

            #[inline]
            fn visit_borrowed(self, cx: &C, bytes: &'de [u8]) -> Result<Self::Ok, C::Error> {
                let string = musli_common::str::from_utf8(bytes).map_err(cx.map())?;
                self.0.visit_borrowed(cx, string)
            }

            #[inline]
            fn visit_ref(self, cx: &C, bytes: &[u8]) -> Result<Self::Ok, C::Error> {
                let string = musli_common::str::from_utf8(bytes).map_err(cx.map())?;
                self.0.visit_ref(cx, string)
            }
        }

        self.decode_bytes(Visitor(visitor))
    }

    #[inline]
    fn decode_bool(mut self) -> Result<bool, C::Error> {
        let mark = self.cx.mark();
        let byte = self.reader.read_byte(self.cx)?;

        match byte {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(self.cx.marked_message(mark, BadBoolean { actual: b })),
        }
    }

    #[inline]
    fn decode_char(self) -> Result<char, C::Error> {
        let cx = self.cx;
        let mark = self.cx.mark();
        let num = self.decode_u32()?;

        match char::from_u32(num) {
            Some(d) => Ok(d),
            None => Err(cx.marked_message(mark, BadCharacter { actual: num })),
        }
    }

    #[inline]
    fn decode_u8(mut self) -> Result<u8, C::Error> {
        self.reader.read_byte(self.cx)
    }

    #[inline]
    fn decode_u16(self) -> Result<u16, C::Error> {
        musli_common::int::decode_unsigned::<_, _, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_u32(self) -> Result<u32, C::Error> {
        musli_common::int::decode_unsigned::<_, _, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_u64(self) -> Result<u64, C::Error> {
        musli_common::int::decode_unsigned::<_, _, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_u128(self) -> Result<u128, C::Error> {
        musli_common::int::decode_unsigned::<_, _, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_i8(self) -> Result<i8, C::Error> {
        Ok(self.decode_u8()? as i8)
    }

    #[inline]
    fn decode_i16(self) -> Result<i16, C::Error> {
        musli_common::int::decode_signed::<_, _, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_i32(self) -> Result<i32, C::Error> {
        musli_common::int::decode_signed::<_, _, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_i64(self) -> Result<i64, C::Error> {
        musli_common::int::decode_signed::<_, _, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_i128(self) -> Result<i128, C::Error> {
        musli_common::int::decode_signed::<_, _, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_usize(self) -> Result<usize, C::Error> {
        musli_common::int::decode_usize::<_, _, F>(self.cx, self.reader)
    }

    #[inline]
    fn decode_isize(self) -> Result<isize, C::Error> {
        Ok(self.decode_usize()? as isize)
    }

    /// Decode a 32-bit floating point value by reading the 32-bit in-memory
    /// IEEE 754 encoding byte-by-byte.
    #[inline]
    fn decode_f32(self) -> Result<f32, C::Error> {
        Ok(f32::from_bits(self.decode_u32()?))
    }

    /// Decode a 64-bit floating point value by reading the 64-bit in-memory
    /// IEEE 754 encoding byte-by-byte.
    #[inline]
    fn decode_f64(self) -> Result<f64, C::Error> {
        let bits = self.decode_u64()?;
        Ok(f64::from_bits(bits))
    }

    #[inline]
    fn decode_option(mut self) -> Result<Option<Self::DecodeSome>, C::Error> {
        let b = self.reader.read_byte(self.cx)?;
        Ok(if b == 1 { Some(self) } else { None })
    }

    #[inline]
    fn decode_sequence(self) -> Result<Self::DecodeSequence, C::Error> {
        LimitedStorageDecoder::new(self)
    }

    #[inline]
    fn decode_tuple(self, _: usize) -> Result<Self::DecodeTuple, C::Error> {
        Ok(self)
    }

    #[inline]
    fn decode_map(self) -> Result<Self::DecodeMap, C::Error> {
        LimitedStorageDecoder::new(self)
    }

    #[inline]
    fn decode_struct(self, _: Option<usize>) -> Result<Self::DecodeStruct, C::Error> {
        LimitedStorageDecoder::new(self)
    }

    #[inline]
    fn decode_variant(self) -> Result<Self::DecodeVariant, C::Error> {
        Ok(self)
    }
}

impl<'a, 'de, R, const F: Options, C: ?Sized + Context> PackDecoder<'de>
    for StorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeNext<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;

    #[inline]
    fn decode_next(&mut self) -> Result<Self::DecodeNext<'_>, C::Error> {
        Ok(StorageDecoder::new(self.cx, self.reader.borrow_mut()))
    }

    #[inline]
    fn end(self) -> Result<(), C::Error> {
        Ok(())
    }
}

impl<'a, 'de, R, const F: Options, C> LimitedStorageDecoder<'a, R, F, C>
where
    C: ?Sized + Context,
    R: Reader<'de>,
{
    #[inline]
    fn new(mut decoder: StorageDecoder<'a, R, F, C>) -> Result<Self, C::Error> {
        let remaining =
            musli_common::int::decode_usize::<_, _, F>(decoder.cx, &mut decoder.reader)?;
        Ok(Self { remaining, decoder })
    }
}

impl<'a, 'de, R, const F: Options, C: ?Sized + Context> SequenceDecoder<'de>
    for LimitedStorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeNext<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::Exact(self.remaining)
    }

    #[inline]
    fn decode_next(&mut self) -> Result<Option<Self::DecodeNext<'_>>, C::Error> {
        if self.remaining == 0 {
            return Ok(None);
        }

        self.remaining -= 1;

        Ok(Some(StorageDecoder::new(
            self.decoder.cx,
            self.decoder.reader.borrow_mut(),
        )))
    }
}

#[musli::map_decoder]
impl<'a, 'de, R, const F: Options, C: ?Sized + Context> MapDecoder<'de>
    for LimitedStorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeEntry<'this> = StorageDecoder<'a, R::Mut<'this>, F, C>
    where
        Self: 'this;
    type IntoMapEntries = Self;

    #[inline]
    fn cx(&self) -> &C {
        self.decoder.cx
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::Exact(self.remaining)
    }

    #[inline]
    fn into_map_entries(self) -> Result<Self::IntoMapEntries, C::Error> {
        Ok(self)
    }

    #[inline]
    fn decode_entry(&mut self) -> Result<Option<Self::DecodeEntry<'_>>, C::Error> {
        if self.remaining == 0 {
            return Ok(None);
        }

        self.remaining -= 1;
        Ok(Some(StorageDecoder::new(
            self.decoder.cx,
            self.decoder.reader.borrow_mut(),
        )))
    }
}

impl<'a, 'de, R, const F: Options, C: ?Sized + Context> MapEntryDecoder<'de>
    for StorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeMapKey<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;
    type DecodeMapValue = Self;

    #[inline]
    fn decode_map_key(&mut self) -> Result<Self::DecodeMapKey<'_>, C::Error> {
        Ok(StorageDecoder::new(self.cx, self.reader.borrow_mut()))
    }

    #[inline]
    fn decode_map_value(self) -> Result<Self::DecodeMapValue, C::Error> {
        Ok(self)
    }

    #[inline]
    fn skip_map_value(self) -> Result<bool, C::Error> {
        Ok(false)
    }
}

#[musli::struct_decoder]
impl<'a, 'de, R, const F: Options, C: ?Sized + Context> StructDecoder<'de>
    for LimitedStorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeField<'this> = StorageDecoder<'a, R::Mut<'this>, F, C>
    where
        Self: 'this;

    type IntoStructFields = Self;

    #[inline]
    fn cx(&self) -> &C {
        self.decoder.cx
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        MapDecoder::size_hint(self)
    }

    #[inline]
    fn into_struct_fields(self) -> Result<Self::IntoStructFields, C::Error> {
        Ok(self)
    }

    #[inline]
    fn decode_field(&mut self) -> Result<Option<Self::DecodeField<'_>>, C::Error> {
        MapDecoder::decode_entry(self)
    }

    #[inline]
    fn end(self) -> Result<(), C::Error> {
        MapDecoder::end(self)
    }
}

impl<'a, 'de, R, const F: Options, C: ?Sized + Context> StructFieldDecoder<'de>
    for StorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeFieldName<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;
    type DecodeFieldValue = Self;

    #[inline]
    fn decode_field_name(&mut self) -> Result<Self::DecodeFieldName<'_>, C::Error> {
        MapEntryDecoder::decode_map_key(self)
    }

    #[inline]
    fn decode_field_value(self) -> Result<Self::DecodeFieldValue, C::Error> {
        MapEntryDecoder::decode_map_value(self)
    }

    #[inline]
    fn skip_field_value(self) -> Result<bool, C::Error> {
        MapEntryDecoder::skip_map_value(self)
    }
}

impl<'a, 'de, R, const F: Options, C: ?Sized + Context> MapEntriesDecoder<'de>
    for LimitedStorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeMapEntryKey<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;
    type DecodeMapEntryValue<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;

    #[inline]
    fn decode_map_entry_key(&mut self) -> Result<Option<Self::DecodeMapEntryKey<'_>>, C::Error> {
        if self.remaining == 0 {
            return Ok(None);
        }

        self.remaining -= 1;
        Ok(Some(StorageDecoder::new(
            self.decoder.cx,
            self.decoder.reader.borrow_mut(),
        )))
    }

    #[inline]
    fn decode_map_entry_value(&mut self) -> Result<Self::DecodeMapEntryValue<'_>, C::Error> {
        Ok(StorageDecoder::new(
            self.decoder.cx,
            self.decoder.reader.borrow_mut(),
        ))
    }

    #[inline]
    fn skip_map_entry_value(&mut self) -> Result<bool, C::Error> {
        Ok(false)
    }
}

impl<'a, 'de, R, const F: Options, C: ?Sized + Context> StructFieldsDecoder<'de>
    for LimitedStorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeStructFieldName<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;
    type DecodeStructFieldValue<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;

    #[inline]
    fn decode_struct_field_name(&mut self) -> Result<Self::DecodeStructFieldName<'_>, C::Error> {
        if self.remaining == 0 {
            return Err(self
                .decoder
                .cx
                .message("Ran out of struct fields to decode"));
        }

        self.remaining -= 1;
        Ok(StorageDecoder::new(
            self.decoder.cx,
            self.decoder.reader.borrow_mut(),
        ))
    }

    #[inline]
    fn decode_struct_field_value(&mut self) -> Result<Self::DecodeStructFieldValue<'_>, C::Error> {
        Ok(StorageDecoder::new(
            self.decoder.cx,
            self.decoder.reader.borrow_mut(),
        ))
    }

    #[inline]
    fn skip_struct_field_value(&mut self) -> Result<bool, C::Error> {
        Ok(false)
    }

    #[inline]
    fn end(self) -> Result<(), C::Error> {
        Ok(())
    }
}

impl<'a, 'de, R, const F: Options, C: ?Sized + Context> VariantDecoder<'de>
    for StorageDecoder<'a, R, F, C>
where
    R: Reader<'de>,
{
    type Cx = C;
    type DecodeTag<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;
    type DecodeVariant<'this> = StorageDecoder<'a, R::Mut<'this>, F, C> where Self: 'this;

    #[inline]
    fn decode_tag(&mut self) -> Result<Self::DecodeTag<'_>, C::Error> {
        Ok(StorageDecoder::new(self.cx, self.reader.borrow_mut()))
    }

    #[inline]
    fn decode_value(&mut self) -> Result<Self::DecodeVariant<'_>, C::Error> {
        Ok(StorageDecoder::new(self.cx, self.reader.borrow_mut()))
    }

    #[inline]
    fn skip_value(&mut self) -> Result<bool, C::Error> {
        Ok(false)
    }

    #[inline]
    fn end(self) -> Result<(), C::Error> {
        Ok(())
    }
}

struct ExpectedEmptySequence {
    actual: usize,
}

impl fmt::Display for ExpectedEmptySequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { actual } = *self;
        write!(f, "Expected empty sequence, but was {actual}",)
    }
}

struct BadBoolean {
    actual: u8,
}

impl fmt::Display for BadBoolean {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { actual } = *self;
        write!(f, "Bad boolean byte 0x{actual:02x}")
    }
}

struct BadCharacter {
    actual: u32,
}

impl fmt::Display for BadCharacter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { actual } = *self;
        write!(f, "Bad character number {actual}")
    }
}
