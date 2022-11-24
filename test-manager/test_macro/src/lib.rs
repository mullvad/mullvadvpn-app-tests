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
    let mut test_function_priority = vec![];
    let mut test_function_mullvad_version = vec![];
    let mut test_wrapper_fn = vec![];

    for func in &test_functions {
        test_function_priority.push(
            func.macro_parameters
                .priority
                .as_ref()
                .map(|priority| {
                    quote! {
                        Some(#priority)
                    }
                })
                .unwrap_or(quote! {None}),
        );

        let func_name = func.name.clone();
        let func_wrapper_name = format_ident!("{}_wrapper", func_name);

        if let Some(mullvad_client_type) = func.function_parameters.mullvad_client_type.clone() {
            test_wrapper_fn.push(quote! {
                fn #func_wrapper_name(
                    rpc: test_rpc::ServiceClient,
                    mullvad_client: Box<dyn std::any::Any>,
                ) -> futures::future::BoxFuture<'static, Result<(), Error>> {
                    use std::any::Any;
                    let mullvad_client = mullvad_client.downcast::<#mullvad_client_type>().expect("invalid mullvad client");
                    Box::pin(#func_name(rpc, *mullvad_client))
                }
            });
        } else {
            test_wrapper_fn.push(quote! {
                fn #func_wrapper_name(
                    rpc: test_rpc::ServiceClient,
                    _mullvad_client: Box<dyn std::any::Any>,
                ) -> futures::future::BoxFuture<'static, Result<(), Error>> {
                    Box::pin(#func_name(rpc))
                }
            });
        }

        test_name_wrappers.push(func_wrapper_name);

        test_function_mullvad_version.push(func.function_parameters.mullvad_client_version.clone());
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
                                mullvad_client_version: #test_function_mullvad_version,
                                func: Box::new(Self::#test_name_wrappers),
                                priority: #test_function_priority,
                            }
                        ),*
                    ],
                }
            }

            #(#test_wrapper_fn)*
        }
    };
    tokens
}

struct TestFunction {
    name: syn::Ident,
    function_parameters: FunctionParameters,
    macro_parameters: MacroParameters,
}

struct MacroParameters {
    priority: Option<syn::LitInt>,
}

struct FunctionParameters {
    mullvad_client_type: Option<Box<syn::Type>>,
    mullvad_client_version: proc_macro2::TokenStream,
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
                            let macro_parameters = get_test_macro_parameters(attribute);
                            function.attrs.remove(i);

                            let function_parameters =
                                get_test_function_parameters(&function.sig.inputs);

                            let test_function = TestFunction {
                                name: function.sig.ident.clone(),
                                function_parameters,
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

fn get_test_macro_parameters(attribute: &syn::Attribute) -> MacroParameters {
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

fn get_test_function_parameters(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>,
) -> FunctionParameters {
    if inputs.len() > 1 {
        match inputs[1].clone() {
            syn::FnArg::Typed(pat_type) => {
                let mullvad_client_version = match &*pat_type.ty {
                    syn::Type::Path(syn::TypePath { path, .. }) => {
                        match path.segments[0].ident.to_string().as_str() {
                            "mullvad_management_interface" | "ManagementServiceClient" => {
                                quote! { test_rpc::mullvad_daemon::MullvadClientVersion::New }
                            }
                            "old_mullvad_management_interface" => {
                                quote! { test_rpc::mullvad_daemon::MullvadClientVersion::Previous }
                            }
                            _ => panic!("cannot infer mullvad client type"),
                        }
                    }
                    _ => panic!("unexpected 'mullvad_client' type"),
                };

                FunctionParameters {
                    mullvad_client_type: Some(pat_type.ty),
                    mullvad_client_version,
                }
            }
            syn::FnArg::Receiver(_) => panic!("unexpected 'mullvad_client' arg"),
        }
    } else {
        FunctionParameters {
            mullvad_client_type: None,
            mullvad_client_version: quote! { test_rpc::mullvad_daemon::MullvadClientVersion::None },
        }
    }
}
