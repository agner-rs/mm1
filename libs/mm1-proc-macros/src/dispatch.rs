use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{ToTokens, quote};
use syn::{Arm, Expr, Pat, parse_macro_input};

pub(crate) fn dispatch(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as Expr);

    expand_dispatch(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_dispatch(input: Expr) -> syn::Result<proc_macro2::TokenStream> {
    let Expr::Match(expr_match) = input else {
        return Err(syn::Error::new_spanned(input, "Expected a match statement"));
    };

    // Extract the match expression arms and the expression being matched
    let arms = expr_match.arms;
    let expr = expr_match.expr;

    let expr_var_name = Ident::new("__expr", Span::mixed_site());

    // Fallback for a message no arm matched: log it and drop it (evaluates
    // to `()`), instead of panicking and killing the actor (#134). A
    // `dispatch!` used in value position must therefore supply its own
    // catch-all arm.
    let match_tree = arms.iter().rev().try_fold(
        quote! {
            #expr_var_name.log_unhandled()
        },
        |or_else, arm| dispatch_arm(&expr_var_name, arm, or_else),
    )?;

    // Generate the output code for dispatch
    Ok(quote! {
        {
            // let #expr_var_name: ::mm1_core::envelope::Envelope::<
            //                         ::mm1_core::message::AnyMessage
            //                     > = #expr;
            let #expr_var_name = #expr;

            #match_tree
        }
    })
}

// attrs — are not supported, should be empty
// guard — if a guard is present, the check should be done with `peek` first,
// only then we do `cast`;
fn dispatch_arm(
    expr_var_name: &Ident,
    arm: &Arm,
    or_else: impl ToTokens,
) -> syn::Result<proc_macro2::TokenStream> {
    let arm_body = &arm.body;
    let pat = &arm.pat;
    let guard = &arm.guard;

    let ArmBind { ty, bind } = dispatch_arm_bind(pat)?;

    if let Some((_if, guard)) = guard {
        let Some(ty) = ty else {
            return Err(syn::Error::new_spanned(
                pat,
                "`dispatch!` does not support guards on catch-all arms",
            ));
        };

        Ok(quote! {
            if #expr_var_name .peek::<#ty>()
                .is_some_and(|#[allow(unused)] #bind| #guard)
            {
                let #expr_var_name = #expr_var_name .cast::<#ty>().expect("peek with the same type has just succeeded");
                #[allow(unused)]  let (#bind, #expr_var_name) = #expr_var_name .take();
                #arm_body
            } else {
                #or_else
            }
        })
    } else if let Some(ty) = ty {
        Ok(quote! {
            match #expr_var_name .cast::<#ty>() {
                Ok(#expr_var_name) => {
                    let (#bind, #expr_var_name) = #expr_var_name .take();
                    #arm_body
                }
                Err(#expr_var_name) => #or_else
            }
        })
    } else {
        Ok(quote! {
            {
                let (#bind, #expr_var_name) = #expr_var_name .take();
                #arm_body
            }
        })
    }
}

struct ArmBind {
    ty:   Option<proc_macro2::TokenStream>,
    bind: proc_macro2::TokenStream,
}

fn dispatch_arm_bind(pat: &Pat) -> syn::Result<ArmBind> {
    match pat {
        Pat::Ident(ident) => {
            match ident.subpat.as_ref() {
                Some((_at_sign, pat_wild)) if matches!(**pat_wild, Pat::Wild { .. }) => {
                    let ident_ident = &ident.ident;
                    Ok(ArmBind {
                        ty:   None,
                        bind: quote! { #ident_ident },
                    })
                },
                Some((at_sign, subpat)) => {
                    let ArmBind { ty, bind } = dispatch_arm_bind(subpat)?;
                    let ident_ident = &ident.ident;
                    Ok(ArmBind {
                        ty,
                        bind: quote! { #ident_ident #at_sign #bind },
                    })
                },
                None if looks_like_binding(&ident.ident) => {
                    Err(syn::Error::new_spanned(
                        ident,
                        "`dispatch!` does not support bare binding catch-alls; write `name @ _` \
                         instead",
                    ))
                },
                None => {
                    Ok(ArmBind {
                        ty:   Some(quote! { #ident }),
                        bind: quote! { #ident },
                    })
                },
            }
        },

        Pat::Path(path) => {
            Ok(ArmBind {
                ty:   Some(quote! { #path }),
                bind: quote! { #path },
            })
        },

        Pat::TupleStruct(tuple_struct) => {
            let ty = &tuple_struct.path;
            let bind = pat;
            Ok(ArmBind {
                ty:   Some(quote! { #ty }),
                bind: quote! { #bind },
            })
        },

        Pat::Struct(normal_struct) => {
            let ty = &normal_struct.path;
            let bind = pat;
            Ok(ArmBind {
                ty:   Some(quote! { #ty }),
                bind: quote! { #bind },
            })
        },

        Pat::Wild(wild) => {
            Ok(ArmBind {
                ty:   None,
                bind: quote! { #wild },
            })
        },

        unsupported => {
            Err(syn::Error::new_spanned(
                unsupported,
                "unsupported pattern in `dispatch!`",
            ))
        },
    }
}

fn looks_like_binding(ident: &Ident) -> bool {
    let ident = ident.to_string();
    let ident = ident.strip_prefix("r#").unwrap_or(&ident);
    let mut ident = ident.trim_start_matches('_').chars();

    ident.next().is_none_or(char::is_lowercase)
}
