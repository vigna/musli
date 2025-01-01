use core::ops::{Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};

use crate::en::SequenceEncoder;
use crate::hint::SequenceHint;
use crate::{Decode, Decoder, Encode, Encoder};

macro_rules! implement {
    ($ty:ident $(<$type:ident>)? { $($field:ident),* }, $count:expr) => {
        impl<M, $($type)*> Encode<M> for $ty $(<$type>)*
        where
            $($type: Encode<M>,)*
        {
            const IS_BITWISE_ENCODE: bool = false;

            #[inline]
            #[allow(unused)]
            fn encode<E>(&self, encoder: E) -> Result<E::Ok, E::Error>
            where
                E: Encoder<Mode = M>,
            {
                static HINT: SequenceHint = SequenceHint::with_size($count);

                encoder.encode_sequence_fn(&HINT, |tuple| {
                    $(tuple.encode_next()?.encode(&self.$field)?;)*
                    Ok(())
                })
            }

            type Encode = Self;

            #[inline]
            fn as_encode(&self) -> &Self::Encode {
                self
            }
        }

        impl<'de, M, $($type)*> Decode<'de, M> for $ty $(<$type>)*
        where
            $($type: Decode<'de, M>,)*
        {
            const IS_BITWISE_DECODE: bool = false;

            #[inline]
            fn decode<D>(decoder: D) -> Result<Self, D::Error>
            where
                D: Decoder<'de, Mode = M>,
            {
                let ($($field,)*) = decoder.decode()?;
                Ok($ty { $($field,)* })
            }
        }
    }
}

macro_rules! implement_new {
    ($ty:ident { $($field:ident),* }, $count:expr) => {
        impl<M, T> Encode<M> for $ty<T>
        where
            T: Encode<M>,
        {
            const IS_BITWISE_ENCODE: bool = false;

            #[inline]
            fn encode<E>(&self, encoder: E) -> Result<E::Ok, E::Error>
            where
                E: Encoder<Mode = M>,
            {
                static HINT: SequenceHint = SequenceHint::with_size($count);

                encoder.encode_sequence_fn(&HINT, |tuple| {
                    $(tuple.encode_next()?.encode(self.$field())?;)*
                    Ok(())
                })
            }

            type Encode = Self;

            #[inline]
            fn as_encode(&self) -> &Self::Encode {
                self
            }
        }

        impl<'de, M, T> Decode<'de, M> for $ty<T>
        where
            T: Decode<'de, M>,
        {
            const IS_BITWISE_DECODE: bool = false;

            #[inline]
            fn decode<D>(decoder: D) -> Result<Self, D::Error>
            where
                D: Decoder<'de, Mode = M>,
            {
                let ($($field,)*) = Decode::decode(decoder)?;
                Ok($ty::new($($field,)*))
            }
        }
    }
}

implement!(RangeFull {}, 0);
implement!(Range<T> { start, end }, 2);
implement!(RangeFrom<T> { start }, 1);
implement!(RangeTo<T> { end }, 1);
implement!(RangeToInclusive<T> { end }, 1);
implement_new!(RangeInclusive { start, end }, 2);
