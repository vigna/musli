use std::rc::Rc;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::punctuated::Punctuated;
use syn::Token;

use crate::internals::attr::{EnumTagging, Packing};
use crate::internals::build::{Body, Build, BuildData, Enum, Field, Variant};
use crate::internals::{Result, Tokens};

struct Ctxt<'a> {
    ctx_var: &'a syn::Ident,
    encoder_var: &'a syn::Ident,
    trace: bool,
}

pub(crate) fn expand_insert_entry(e: Build<'_, '_>) -> Result<TokenStream> {
    e.validate_encode()?;
    e.cx.reset();

    let type_ident = &e.input.ident;

    let encoder_var = e.cx.ident("encoder");
    let ctx_var = e.cx.ident("ctx");
    let e_param = e.cx.type_with_span("E", Span::call_site());

    let cx = Ctxt {
        ctx_var: &ctx_var,
        encoder_var: &encoder_var,
        trace: true,
    };

    let Tokens {
        context_t,
        encode_t,
        encoder_t,
        option,
        result,
        try_fast_encode,
        ..
    } = e.tokens;

    let packed;

    let (body, size_hint) = match &e.data {
        BuildData::Struct(st) => {
            packed = crate::internals::packed(&e, st);
            encode_map(&cx, &e, st)?
        }
        BuildData::Enum(en) => {
            packed = syn::parse_quote!(false);
            encode_enum(&cx, &e, en)?
        }
    };

    if e.cx.has_errors() {
        return Err(());
    }

    let mut impl_generics = e.input.generics.clone();

    if !e.bounds.is_empty() {
        let where_clause = impl_generics.make_where_clause();

        where_clause
            .predicates
            .extend(e.bounds.iter().map(|(_, v)| v.clone()));
    }

    let (impl_generics, _, where_clause) = impl_generics.split_for_impl();
    let (_, type_generics, _) = e.input.generics.split_for_impl();

    let mut attributes = Vec::<syn::Attribute>::new();

    if cfg!(not(feature = "verbose")) {
        attributes.push(syn::parse_quote!(#[allow(clippy::just_underscores_and_digits)]));
    }

    let mode_ident = e.expansion.mode_path(e.tokens);

    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            #(#attributes)*
            impl #impl_generics #encode_t<#mode_ident> for #type_ident #type_generics
            #where_clause
            {
                type Encode = Self;

                const IS_BITWISE_ENCODE: bool = #packed;

                #[inline]
                fn encode<#e_param>(&self, #encoder_var: #e_param) -> #result<(), <#e_param as #encoder_t>::Error>
                where
                    #e_param: #encoder_t<Mode = #mode_ident>,
                {
                    let #ctx_var = #encoder_t::cx(&#encoder_var);

                    let #encoder_var = match #encoder_t::try_fast_encode(#encoder_var, self)? {
                        #try_fast_encode::Ok => return #result::Ok(()),
                        #try_fast_encode::Unsupported(_, #encoder_var) => #encoder_var,
                        _ => return #result::Err(#context_t::message(#ctx_var, "Fast encoding failed")),
                    };

                    #body
                }

                #[inline]
                fn size_hint(&self) -> #option<usize> {
                    #size_hint
                }

                #[inline]
                fn as_encode(&self) -> &Self::Encode {
                    self
                }
            }
        };
    })
}

/// Encode a struct.
fn encode_map(
    cx: &Ctxt<'_>,
    b: &Build<'_, '_>,
    st: &Body<'_>,
) -> Result<(TokenStream, TokenStream)> {
    let Ctxt {
        ctx_var,
        encoder_var,
        ..
    } = *cx;

    let Tokens {
        context_t,
        encoder_t,
        option,
        result,
        ..
    } = b.tokens;

    let pack_var = b.cx.ident("pack");
    let output_var = b.cx.ident("output");

    let encoders = make_encoders(cx, b, st, &pack_var);
    let decls = make_field_tests(st);

    let type_name = &st.name;

    let enter = cx
        .trace
        .then(|| quote!(#context_t::enter_struct(#ctx_var, #type_name);));
    let leave = cx
        .trace
        .then(|| quote!(#context_t::leave_struct(#ctx_var);));

    let encode;
    let size_hint;

    match st.packing {
        Packing::Transparent => {
            let f = &st.unskipped_fields[0];

            let access = &f.self_access;
            let encode_path = &f.encode_path.1;

            encode = quote! {{
                #enter
                let #output_var = #encode_path(#access, #encoder_var)?;
                #leave
                #output_var
            }};

            size_hint = match &f.size_hint_path {
                Some((_, path)) => quote!(#path(#access)),
                None => quote!(#option::None),
            };
        }
        Packing::Tagged => {
            let len = length_test(&st.unskipped_fields);
            let (build_hint, hint) = len.build_hint(b);

            encode = quote! {{
                #enter
                #(#decls)*
                #build_hint

                let #output_var = #encoder_t::encode_map_fn(#encoder_var, #hint, move |#encoder_var| {
                    #(#encoders)*
                    #result::Ok(())
                })?;

                #leave
                #output_var
            }};

            let len = &len.expressions;

            size_hint = quote! {{
                #(#decls)*
                #option::Some(#len)
            }};
        }
        Packing::Packed => {
            encode = quote! {{
                #enter
                let #output_var = #encoder_t::encode_pack_fn(#encoder_var, move |#pack_var| {
                    #(#decls)*
                    #(#encoders)*
                    #result::Ok(())
                })?;
                #leave
                #output_var
            }};

            size_hint = quote!(#option::None);
        }
    }

    let encode = quote!(#result::Ok(#encode));
    Ok((encode, size_hint))
}

fn make_encoders(
    cx: &Ctxt<'_>,
    b: &Build<'_, '_>,
    st: &Body<'_>,
    pack_var: &syn::Ident,
) -> Vec<TokenStream> {
    let Ctxt {
        ctx_var,
        encoder_var,
        ..
    } = *cx;

    let Tokens {
        context_t,
        sequence_encoder_t,
        result,

        map_encoder_t,
        map_entry_encoder_t,
        ..
    } = b.tokens;

    let encode_t_encode = &b.encode_t_encode;

    let sequence_decoder_next_var = b.cx.ident("sequence_decoder_next");
    let pair_encoder_var = b.cx.ident("pair_encoder");
    let field_encoder_var = b.cx.ident("field_encoder");
    let value_encoder_var = b.cx.ident("value_encoder");
    let field_name_static = b.cx.ident("FIELD_NAME");
    let field_name_expr = st.name_type.expr(field_name_static.clone());

    let mut encoders = Vec::with_capacity(st.all_fields.len());

    for f in &st.unskipped_fields {
        let encode_path = &f.encode_path.1;
        let access = &f.self_access;
        let name = &f.name;
        let name_type = st.name_type.ty();

        let mut encode;

        let enter = match &f.member {
            syn::Member::Named(ident) => {
                let field_name = syn::LitStr::new(&ident.to_string(), ident.span());

                cx.trace.then(|| {
                    let formatted_name = st.name_type.name_format(&field_name_static);

                    quote! {
                        #context_t::enter_named_field(#ctx_var, #field_name, #formatted_name);
                    }
                })
            }
            syn::Member::Unnamed(index) => {
                let index = index.index;
                cx.trace.then(|| {
                    let formatted_name = st.name_type.name_format(&field_name_static);

                    quote! {
                        #context_t::enter_unnamed_field(#ctx_var, #index, #formatted_name);
                    }
                })
            }
        };

        let leave = cx.trace.then(|| quote!(#context_t::leave_field(#ctx_var);));

        match f.packing {
            Packing::Tagged | Packing::Transparent => {
                encode = quote! {{
                    static #field_name_static: #name_type = #name;

                    #enter

                    #map_encoder_t::encode_entry_fn(#encoder_var, move |#pair_encoder_var| {
                        let #field_encoder_var = #map_entry_encoder_t::encode_key(#pair_encoder_var)?;
                        #encode_t_encode(#field_name_expr, #field_encoder_var)?;
                        let #value_encoder_var = #map_entry_encoder_t::encode_value(#pair_encoder_var)?;
                        #encode_path(#access, #value_encoder_var)?;
                        #result::Ok(())
                    })?;

                    #leave
                }};
            }
            Packing::Packed => {
                let decl = enter.is_some().then(|| {
                    quote! {
                        static #field_name_static: #name_type = #name;
                    }
                });

                encode = quote! {{
                    #decl
                    #enter
                    let #sequence_decoder_next_var = #sequence_encoder_t::encode_next(#pack_var)?;
                    #encode_path(#access, #sequence_decoder_next_var)?;
                    #leave
                }};
            }
        };

        if f.skip_encoding_if.is_some() {
            let var = &f.var;

            encode = quote! {
                if #var {
                    #encode
                }
            };
        }

        encoders.push(encode);
    }

    encoders
}

fn make_field_tests(st: &Body<'_>) -> Vec<TokenStream> {
    let mut decls = Vec::with_capacity(st.unskipped_fields.len());

    for f in &st.unskipped_fields {
        if let Some((_, skip_encoding_if_path)) = f.skip_encoding_if.as_ref() {
            let access = &f.self_access;
            let var = &f.var;

            decls.push(quote! {
                let #var = !#skip_encoding_if_path(#access);
            });
        }
    }

    decls
}

/// Encode an internally tagged enum.
fn encode_enum(
    cx: &Ctxt<'_>,
    b: &Build<'_, '_>,
    en: &Enum<'_>,
) -> Result<(TokenStream, TokenStream)> {
    let Ctxt { .. } = *cx;

    let Tokens { result, .. } = b.tokens;

    if let Some(&(span, Packing::Transparent)) = en.packing_span {
        b.encode_transparent_enum_diagnostics(span);
        return Err(());
    }

    let mut encode_variants = Vec::with_capacity(en.variants.len());
    let mut size_hint_variants = Vec::with_capacity(en.variants.len());

    for v in &en.variants {
        let Ok((pattern, encode, size_hint)) = encode_variant(cx, b, en, v) else {
            continue;
        };

        encode_variants.push(quote!(#pattern => #encode));
        size_hint_variants.push(quote!(#pattern => #size_hint));
    }

    let encode;
    let size_hint;

    if encode_variants.is_empty() {
        encode = quote!(match *self {});
        size_hint = quote!(match *self {});
    } else {
        encode = quote! {
            #result::Ok(match *self { #(#encode_variants),* })
        };

        size_hint = quote! {
            match *self { #(#size_hint_variants),* }
        };
    }

    Ok((encode, size_hint))
}

/// Setup encoding for a single variant. that is externally tagged.
fn encode_variant(
    cx: &Ctxt<'_>,
    b: &Build<'_, '_>,
    en: &Enum<'_>,
    v: &Variant<'_>,
) -> Result<(syn::PatStruct, TokenStream, TokenStream)> {
    let Ctxt {
        ctx_var,
        encoder_var,
        ..
    } = *cx;

    let Tokens {
        context_t,
        encoder_t,
        map_encoder_t,
        map_entry_encoder_t,
        option,
        result,
        variant_encoder_t,
        ..
    } = b.tokens;

    let pack_var = b.cx.ident("pack");
    let content_static = b.cx.ident("CONTENT");
    let hint = b.cx.ident("STRUCT_HINT");
    let name_static = b.cx.ident("NAME");
    let name_expr = en.name_type.expr(name_static.clone());
    let tag_encoder = b.cx.ident("tag_encoder");
    let tag_static = b.cx.ident("TAG");
    let variant_encoder = b.cx.ident("variant_encoder");

    let mut encode;
    let size_hint;

    match &en.enum_tagging {
        EnumTagging::Empty => {
            let name_type = en.name_type.ty();
            let encode_t_encode = &b.encode_t_encode;
            let name = &v.name;

            encode = quote! {{
                static #name_static: #name_type = #name;
                #encode_t_encode(#name_expr, #encoder_var)?
            }};

            size_hint = quote!(#option::Some(1));
        }
        EnumTagging::Default => {
            match v.st.packing {
                Packing::Transparent => {
                    let f = &v.st.unskipped_fields[0];

                    let encode_path = &f.encode_path.1;
                    let access = &f.self_access;

                    encode = quote!(#encode_path(#access, #encoder_var)?);

                    size_hint = match &f.size_hint_path {
                        Some((_, path)) => quote!(#path(#access)),
                        None => quote!(#option::None),
                    };
                }
                Packing::Packed => {
                    let encoders = make_encoders(cx, b, &v.st, &pack_var);
                    let decls = make_field_tests(&v.st);

                    encode = quote! {{
                        #encoder_t::encode_pack_fn(#encoder_var, move |#pack_var| {
                            #(#decls)*
                            #(#encoders)*
                            #result::Ok(())
                        })?
                    }};

                    size_hint = quote!(#option::None);
                }
                Packing::Tagged => {
                    let encoders = make_encoders(cx, b, &v.st, &pack_var);
                    let decls = make_field_tests(&v.st);

                    let len = length_test(&v.st.unskipped_fields);
                    let (build_hint, hint) = len.build_hint(b);

                    encode = quote! {{
                        #build_hint

                        #encoder_t::encode_map_fn(#encoder_var, #hint, move |#encoder_var| {
                            #(#decls)*
                            #(#encoders)*
                            #result::Ok(())
                        })?
                    }};

                    let len = &len.expressions;

                    size_hint = quote! {{
                        #(#decls)*
                        #option::Some(#len)
                    }};
                }
            }

            if let Packing::Tagged = en.enum_packing {
                let encode_t_encode = &b.encode_t_encode;
                let name = &v.name;
                let name_type = en.name_type.ty();

                encode = quote! {{
                    #encoder_t::encode_variant_fn(#encoder_var, move |#variant_encoder| {
                        let #tag_encoder = #variant_encoder_t::encode_tag(#variant_encoder)?;
                        static #name_static: #name_type = #name;

                        #encode_t_encode(#name_expr, #tag_encoder)?;

                        let #encoder_var = #variant_encoder_t::encode_data(#variant_encoder)?;
                        #encode;
                        #result::Ok(())
                    })?
                }};
            }
        }
        EnumTagging::Internal {
            tag_value,
            tag_type,
        } => {
            'done: {
                let name = &v.name;
                let name_type = en.name_type.ty();

                let mut len;
                let inner_encode;
                let decls;

                match v.st.packing {
                    Packing::Transparent => {
                        let f = &v.st.unskipped_fields[0];
                        let access = &f.self_access;

                        let Some((_, path)) = &f.size_hint_path else {
                            encode = quote! {
                                return #result::Err(#context_t::message(
                                    #ctx_var,
                                    "Cannot encode transparent field with custom encoding",
                                ));
                            };

                            size_hint = quote!(#option::None);
                            break 'done;
                        };

                        decls = make_field_tests(&v.st);

                        len = LengthTest::default();
                        len.expressions.push(quote!(#path(#access)?));

                        let access = &f.self_access;
                        let encode_path = &f.encode_path.1;

                        inner_encode = quote! {
                            let #encoder_var = #map_encoder_t::as_encoder(#encoder_var);
                            #encode_path(#access, #encoder_var)?;
                        };
                    }
                    _ => {
                        decls = make_field_tests(&v.st);
                        len = length_test(&v.st.unskipped_fields);

                        let encoders = make_encoders(cx, b, &v.st, &pack_var);

                        inner_encode = quote! {
                            #(#decls)*
                            #(#encoders)*
                        };
                    }
                }

                // Add one for the tag field.
                len.expressions.push(quote!(1));

                let (build_hint, hint) = len.build_hint(b);

                let tag_type = tag_type.ty();

                encode = quote! {{
                    #build_hint

                    #encoder_t::encode_map_fn(#encoder_var, #hint, move |#encoder_var| {
                        static #tag_static: #tag_type = #tag_value;
                        static #name_static: #name_type = #name;
                        #map_encoder_t::insert_entry(#encoder_var, #tag_static, #name_static)?;
                        #inner_encode
                        #result::Ok(())
                    })?
                }};

                let len = &len.expressions;

                size_hint = quote! {{
                    #(#decls)*
                    #option::Some(#len)
                }};
            };
        }
        EnumTagging::Adjacent {
            tag_value,
            tag_type,
            content_value,
            content_type,
        } => {
            let encoders = make_encoders(cx, b, &v.st, &pack_var);
            let decls = make_field_tests(&v.st);

            let encode_t_encode = &b.encode_t_encode;

            let name = &v.name;
            let name_type = en.name_type.ty();

            let (build_hint, inner_hint) = length_test(&v.st.unskipped_fields).build_hint(b);
            let struct_encoder = b.cx.ident("struct_encoder");
            let content_struct = b.cx.ident("content_struct");
            let pair = b.cx.ident("pair");
            let content_tag = b.cx.ident("content_tag");

            let tag_type = tag_type.ty();
            let content_static_expr = content_type.expr(content_static.clone());
            let content_type = content_type.ty();

            encode = quote! {{
                static #hint: usize = 2;
                #build_hint

                #encoder_t::encode_map_fn(#encoder_var, #hint, move |#struct_encoder| {
                    static #tag_static: #tag_type = #tag_value;
                    static #content_static: #content_type = #content_value;
                    static #name_static: #name_type = #name;

                    #map_encoder_t::insert_entry(#struct_encoder, #tag_static, #name_static)?;

                    #map_encoder_t::encode_entry_fn(#struct_encoder, move |#pair| {
                        let #content_tag = #map_entry_encoder_t::encode_key(#pair)?;
                        #encode_t_encode(#content_static_expr, #content_tag)?;

                        let #content_struct = #map_entry_encoder_t::encode_value(#pair)?;

                        #encoder_t::encode_map_fn(#content_struct, #inner_hint, move |#encoder_var| {
                            #(#decls)*
                            #(#encoders)*
                            #result::Ok(())
                        })?;

                        #result::Ok(())
                    })?;

                    #result::Ok(())
                })?
            }};

            size_hint = quote!(#option::Some(2));
        }
    }

    let pattern = syn::PatStruct {
        attrs: Vec::new(),
        qself: None,
        path: v.st.path.clone(),
        brace_token: syn::token::Brace::default(),
        fields: v.patterns.clone(),
        rest: None,
    };

    if cx.trace {
        let output_var = b.cx.ident("output");

        let formatted_tag = en.name_type.name_format(&name_static);
        let name_type = en.name_type.ty();
        let name_value = &v.name;
        let type_name = v.st.name;

        encode = quote! {{
            static #name_static: #name_type = #name_value;
            #context_t::enter_variant(#ctx_var, #type_name, #formatted_tag);
            let #output_var = #encode;
            #context_t::leave_variant(#ctx_var);
            #output_var
        }};
    }

    Ok((pattern, encode, size_hint))
}

#[derive(Default)]
struct LengthTest {
    kind: LengthTestKind,
    expressions: Punctuated<TokenStream, Token![+]>,
}

impl LengthTest {
    fn build_hint(&self, b: &Build<'_, '_>) -> (TokenStream, syn::Ident) {
        let Tokens { map_hint, .. } = b.tokens;

        let len = &self.expressions;

        match self.kind {
            LengthTestKind::Static => {
                let hint = b.cx.ident("HINT");
                let item = quote! {
                    static #hint: usize = #len;
                };

                (item, hint)
            }
            LengthTestKind::Dynamic => {
                let mode = &b.mode.mode_path;
                let hint = b.cx.ident("hint");
                let item = quote! {
                    let #hint = #map_hint::<#mode>(self);
                };

                (item, hint)
            }
        }
    }
}

#[derive(Default)]
enum LengthTestKind {
    Static,
    #[default]
    Dynamic,
}

fn length_test(fields: &[Rc<Field<'_>>]) -> LengthTest {
    let mut kind = LengthTestKind::Static;

    let mut expressions = Punctuated::<_, Token![+]>::new();
    let mut count = 0usize;

    for f in fields {
        if f.skip_encoding_if.is_some() {
            let var = &f.var;
            kind = LengthTestKind::Dynamic;
            expressions.push(quote!(if #var { 1 } else { 0 }))
        } else {
            count += 1;
        }
    }

    expressions.push(quote!(#count));

    LengthTest { kind, expressions }
}
