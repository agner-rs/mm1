use either::Either;
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{parse_macro_input, Data, DeriveInput, Fields, GenericParam};

pub(crate) fn derive_traversable(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);

    let traversable_type_name = &input.ident;

    let generic_inspector_type_name = format_ident!("___Inspector{}", traversable_type_name);
    let generic_adjuster_type_name = format_ident!("___Adjuster{}", traversable_type_name);

    let generics_introduction_list = {
        let lifetimes = input.generics.params.iter().filter_map(|g| {
            match g {
                GenericParam::Lifetime(l) => Some(quote! { #l }),
                _ => None,
            }
        });

        let the_rest = input.generics.params.iter().filter_map(|g| {
            match g {
                GenericParam::Lifetime(_l) => None,
                GenericParam::Const(c) => Some(quote! { #c }),
                GenericParam::Type(t) => Some(quote! { #t }),
            }
        });

        lifetimes
            .chain([
                quote! { #generic_inspector_type_name },
                quote! { #generic_adjuster_type_name },
            ])
            .chain(the_rest)
    };
    let generics_application_list = input.generics.params.iter().map(|g| {
        match g {
            GenericParam::Const(c) => {
                let id = &c.ident;
                quote! { #id }
            },
            GenericParam::Lifetime(l) => {
                let lt = &l.lifetime;
                quote! { #lt }
            },
            GenericParam::Type(t) => {
                let id = &t.ident;
                quote! { #id }
            },
        }
    });
    let generics_where_predicates = input
        .generics
        .where_clause
        .iter()
        .flat_map(|wc| wc.predicates.iter())
        .map(|p| p.to_token_stream());

    let traversable_bounds = match &input.data {
        Data::Struct(data_struct) => Some(Either::Left(data_struct.fields.iter().map(|f| &f.ty))),
        Data::Enum(data_enum) => {
            Some(Either::Right(
                data_enum
                    .variants
                    .iter()
                    .flat_map(|variant| variant.fields.iter())
                    .map(|f| &f.ty),
            ))
        },
        Data::Union(_) => None,
    }
    .into_iter()
    .flatten();

    let impl_header = quote! {
        impl
            < #(#generics_introduction_list ,)* >
            ::mm1_proto::Traversable< #generic_inspector_type_name, #generic_adjuster_type_name >
                for
            #traversable_type_name < #(#generics_application_list ,)* >
            where
                #(#traversable_bounds : ::mm1_proto::Traversable< #generic_inspector_type_name, #generic_adjuster_type_name > ,)*
                #(#generics_where_predicates ,)*
    };

    let impl_methods = match &input.data {
        Data::Struct(data_struct) => {
            let bind_fields = impl_bind_fields(true, &data_struct.fields);
            let call_fields = impl_call_fields(quote! { adjust }, &data_struct.fields);
            let fn_adjust = impl_fn_adjust(
                &generic_adjuster_type_name,
                quote! {
                    let #traversable_type_name #bind_fields = self;
                    #call_fields
                },
            );

            let bind_fields = impl_bind_fields(false, &data_struct.fields);
            let call_fields = impl_call_fields(quote! { inspect }, &data_struct.fields);
            let fn_inspect = impl_fn_inspect(
                &generic_inspector_type_name,
                quote! {
                    let #traversable_type_name #bind_fields = self;
                    #call_fields
                },
            );
            quote! {
                #fn_adjust
                #fn_inspect
            }
        },
        Data::Enum(data_enum) => {
            let variants = data_enum.variants.iter().map(|variant| {
                let variant_ident = &variant.ident;
                let bind_fields = impl_bind_fields(true, &variant.fields);
                let call_fields = impl_call_fields(quote! { adjust }, &variant.fields);
                quote! {
                    Self::#variant_ident #bind_fields => {
                        #call_fields
                    },
                }
            });
            let fn_adjust = impl_fn_adjust(
                &generic_adjuster_type_name,
                if data_enum.variants.is_empty() {
                    quote! {
                        match *self { }
                    }
                } else {
                    quote! {
                        match self {
                            #(#variants)*
                        }
                    }
                },
            );

            let variants = data_enum.variants.iter().map(|variant| {
                let variant_ident = &variant.ident;
                let bind_fields = impl_bind_fields(false, &variant.fields);
                let call_fields = impl_call_fields(quote! { inspect }, &variant.fields);
                quote! {
                    Self::#variant_ident #bind_fields => {
                        #call_fields
                    },
                }
            });
            let fn_inspect = impl_fn_inspect(
                &generic_inspector_type_name,
                if data_enum.variants.is_empty() {
                    quote! {
                        match *self { }
                    }
                } else {
                    quote! {
                        match self {
                            #(#variants)*
                        }
                    }
                },
            );

            quote! {
                #fn_adjust
                #fn_inspect
            }
        },
        Data::Union(_) => quote! {},
    };

    let output = quote! {
        // #input

        #impl_header {
            type InspectError = ::mm1_proto::AnyError;
            type AdjustError = ::mm1_proto::AnyError;

            #impl_methods
        }
    };

    // eprintln!("{}", output);

    output.into()
}

fn impl_bind_fields(mutable: bool, fields: &Fields) -> impl ToTokens + '_ {
    let bindings = fields.iter().enumerate().map(|(i, f)| {
        let m = mutable.then_some(quote! {mut});
        let f = if let Some(f) = f.ident.as_ref() {
            f.to_owned()
        } else {
            format_ident!("f_{}", i)
        };

        quote! { #m #f }
    });

    match fields {
        Fields::Unit => quote! {},
        Fields::Named { .. } => {
            quote! {
                { #( ref #bindings , )* }
            }
        },
        Fields::Unnamed { .. } => {
            quote! {
                ( #( ref #bindings , )* )
            }
        },
    }
}

fn impl_call_fields(method: impl ToTokens, fields: &Fields) -> impl ToTokens + '_ {
    let calls = fields.iter().enumerate().map(|(i, f)| {
        let f = if let Some(f) = f.ident.as_ref() {
            f.to_owned()
        } else {
            format_ident!("f_{}", i)
        };
        quote! {
            ::mm1_proto::Traversable:: #method (#f, visitor).map_err(Into::into)?;
        }
    });
    quote! { #(#calls)* }
}

fn impl_fn_inspect(
    generic_inspector_type_name: &proc_macro2::Ident,
    body: impl ToTokens,
) -> impl ToTokens {
    quote! {
        fn inspect(
            &self,
            visitor: &mut #generic_inspector_type_name,
        ) -> Result<
            (),
            Self::InspectError,
        > {
            #body

            #[allow(unreachable_code)]
            Ok(())
        }
    }
}

fn impl_fn_adjust(
    generic_adjuster_type_name: &proc_macro2::Ident,
    body: impl ToTokens,
) -> impl ToTokens {
    quote! {
        fn adjust(
            &mut self,
            visitor: &mut #generic_adjuster_type_name,
        ) -> Result<
            (),
            Self::AdjustError,
        > {
            #body

            #[allow(unreachable_code)]
            Ok(())
        }
    }
}
