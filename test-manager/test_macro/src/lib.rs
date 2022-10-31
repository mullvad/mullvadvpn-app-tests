use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{Lit, Meta, NestedMeta};

#[proc_macro_attribute]
pub fn test_module(_: TokenStream, code: TokenStream) -> TokenStream {
    let mut ast: syn::ItemMod = syn::parse(code).unwrap();

    let marked_functions = parse_marked_test_functions(&mut ast);

    let struct_and_impl = create_test_struct_and_impl(&ast, marked_functions);

    ast.content
        .as_mut()
        .expect("Module must have a body")
        .1
        .push(syn::Item::Verbatim(struct_and_impl));
    ast.into_token_stream().into()
}

fn create_test_struct_and_impl(
    ast: &syn::ItemMod,
    test_functions: Vec<TestFunction>,
) -> proc_macro2::TokenStream {
    let module_name = &ast.ident;

    let mut test_name_wrappers = vec![];

    let test_function_names: Vec<_> = test_functions.iter().map(|f| &f.name).collect();
    let test_function_priority: Vec<_> = test_functions
        .iter()
        .map(|f| {
            f.macro_parameters
                .priority
                .as_ref()
                .map(|priority| {
                    quote! {
                        Some(#priority)
                    }
                })
                .unwrap_or(quote! {None})
        })
        .collect();

    for test_func in &test_functions {
        test_name_wrappers.push(format_ident!("{}_wrapper", test_func.name));
    }
    let struct_name = format_ident!("{}", module_name.to_string().to_case(Case::Pascal));

    let tokens = quote! {
        pub struct #struct_name {
            /// Vec of a struct defined by the calling library
            pub tests: Vec<crate::tests::test_metadata::TestMetadata>
        }

        impl #struct_name {
            pub fn new() -> Self {
                Self {
                    tests: vec![
                        #(
                            crate::tests::test_metadata::TestMetadata {
                                name: stringify!(#test_function_names),
                                command: stringify!(#test_function_names),
                                func: Box::new(Self::#test_name_wrappers),
                                priority: #test_function_priority
                            }
                        ),*
                    ],
                }
            }

            #(fn #test_name_wrappers(rpc: test_rpc::ServiceClient) -> futures::future::BoxFuture<'static, Result<(), Error>> {
                Box::pin(#test_function_names(rpc))
            })*

        }
    };
    tokens
}

struct TestFunction {
    name: syn::Ident,
    macro_parameters: MacroParameters,
}

struct MacroParameters {
    priority: Option<syn::LitInt>,
}

fn get_test_parameters(attribute: &syn::Attribute) -> MacroParameters {
    let mut priority = None;
    if let Ok(Meta::List(list)) = attribute.parse_meta() {
        for meta in list.nested {
            if let NestedMeta::Meta(Meta::NameValue(nv)) = meta {
                if nv.path.is_ident("priority") {
                    match nv.lit {
                        Lit::Int(lit_int) => {
                            priority = Some(lit_int);
                        }
                        _ => panic!("'priority' should have an integer value"),
                    }
                }
            }
        }
    }

    MacroParameters { priority }
}

fn parse_marked_test_functions(ast: &mut syn::ItemMod) -> Vec<TestFunction> {
    match &mut ast.content {
        None => vec![],
        Some((_, items)) => {
            let mut test_functions = vec![];
            for item in items {
                if let syn::Item::Fn(function) = item {
                    for i in 0..function.attrs.len() {
                        let attribute = &function.attrs[i];
                        if attribute.path.is_ident("manager_test") {
                            let macro_parameters = get_test_parameters(attribute);
                            function.attrs.remove(i);

                            let test_function = TestFunction {
                                name: function.sig.ident.clone(),
                                macro_parameters,
                            };
                            test_functions.push(test_function);
                            break;
                        }
                    }
                }
            }
            test_functions
        }
    }
}
