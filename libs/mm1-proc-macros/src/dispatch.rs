use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{ToTokens, quote};
use syn::{Arm, Expr, Pat, parse_macro_input};

pub(crate) fn dispatch(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as Expr);

    let output = if let Expr::Match(expr_match) = input {
        // Extract the match expression arms and the expression being matched
        let arms = expr_match.arms;
        let expr = expr_match.expr;

        let expr_var_name = Ident::new("__expr", Span::mixed_site());

        let match_tree = arms.iter().rev().fold(
            quote! {
                panic!(
                    "unhandled message: header: {:?}, type {}",
                    #expr_var_name.header(),
                    #expr_var_name.message_name()
                )
            },
            |or_else, arm| dispatch_arm(&expr_var_name, arm, or_else),
        );

        // Generate the output code for dispatch
        quote! {
            {
                // let #expr_var_name: ::mm1_core::envelope::Envelope::<
                //                         ::mm1_core::message::AnyMessage
                //                     > = #expr;
                let #expr_var_name = #expr;

                #match_tree
            }
        }
    } else {
        // If input is not a match expression, return a compile error
        return syn::Error::new_spanned(input, "Expected a match statement")
            .to_compile_error()
            .into();
    };

    // eprintln!("{}", output);

    output.into()
}

// attrs — are not supported, should be empty
// guard — if a guard is present, the check should be done with `peek` first,
// only then we do `cast`;
fn dispatch_arm(
    expr_var_name: &Ident,
    arm: &Arm,
    or_else: impl ToTokens,
) -> proc_macro2::TokenStream {
    let arm_body = &arm.body;
    let pat = &arm.pat;
    let guard = &arm.guard;

    let ArmBind { ty, bind } = dispatch_arm_bind(pat);

    if let Some((_if, guard)) = guard {
        quote! {
            if #expr_var_name .peek::<#ty>()
                .is_some_and(|#[allow(unused)] #bind| #guard)
            {
                let #expr_var_name = #expr_var_name .cast::<#ty>().expect("peek with the same type has just succeeded");
                #[allow(unused)]  let (#bind, #expr_var_name) = #expr_var_name .take();
                #arm_body
            } else {
                #or_else
            }
        }
    } else if let Some(ty) = ty {
        quote! {
            match #expr_var_name .cast::<#ty>() {
                Ok(#expr_var_name) => {
                    let (#bind, #expr_var_name) = #expr_var_name .take();
                    #arm_body
                }
                Err(#expr_var_name) => #or_else
            }
        }
    } else {
        quote! {
            {
                let (#bind, #expr_var_name) = #expr_var_name .take();
                #arm_body
            }
        }
    }
}

struct ArmBind {
    ty:   Option<proc_macro2::TokenStream>,
    bind: proc_macro2::TokenStream,
}

fn dispatch_arm_bind(pat: &Pat) -> ArmBind {
    match pat {
        Pat::Ident(ident) => {
            match ident.subpat.as_ref() {
                Some((_at_sign, pat_wild)) if matches!(**pat_wild, Pat::Wild { .. }) => {
                    let ident_ident = &ident.ident;
                    ArmBind {
                        ty:   None,
                        bind: quote! { #ident_ident },
                    }
                },
                Some((at_sign, subpat)) => {
                    let ArmBind { ty, bind } = dispatch_arm_bind(subpat);
                    let ident_ident = &ident.ident;
                    ArmBind {
                        ty,
                        bind: quote! { #ident_ident #at_sign #bind },
                    }
                },
                None => {
                    ArmBind {
                        ty:   Some(quote! { #ident }),
                        bind: quote! { #ident },
                    }
                },
            }
        },

        Pat::Path(path) => {
            ArmBind {
                ty:   Some(quote! { #path }),
                bind: quote! { #path },
            }
        },

        Pat::TupleStruct(tuple_struct) => {
            let ty = &tuple_struct.path;
            let bind = pat;
            ArmBind {
                ty:   Some(quote! { #ty }),
                bind: quote! { #bind },
            }
        },

        Pat::Struct(normal_struct) => {
            let ty = &normal_struct.path;
            let bind = pat;
            ArmBind {
                ty:   Some(quote! { #ty }),
                bind: quote! { #bind },
            }
        },

        unsupported => {
            // eprintln!("UNSUPPORTED: {:?}", unsupported);
            let error =
                syn::Error::new_spanned(unsupported, "UNSUPPORTED PATTERN").to_compile_error();
            ArmBind {
                ty:   Some(quote! { #error }),
                bind: quote! {},
            }
        },
    }
}
