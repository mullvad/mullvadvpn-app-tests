//! Use this crate as such with the following attribute macro above test functions.
//! #[test_function]
//! pub async fn test_function(
//!     rpc: ServiceClient,
//!     mut mullvad_client: mullvad_management_interface::ManagementServiceClient,
//! ) -> Result<(), Error> {
//! The `mullvad_client` argument can be removed.
//! The `test_function` macro takes two optional arguments
//! #[test_function(priority = -1337, cleanup = false)]
//! Priority defaults to 0 and cleanup defaults to true. Priority is the order in which tests will
//! be run where low numbers run before high numbers and tests with the same number run in
//! undefined order. Cleanup means that the cleanup function will run after the test is finished
//! and among other things reset the settings to the default value for the daemon.
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{AttributeArgs, Lit, Meta, NestedMeta};

#[proc_macro_attribute]
pub fn test_function(attributes: TokenStream, code: TokenStream) -> TokenStream {
    let function: syn::ItemFn = syn::parse(code).unwrap();
    let attributes = syn::parse_macro_input!(attributes as AttributeArgs);

    let test_function = parse_marked_test_function(&attributes, &function);

    let register_test = create_test(test_function);

    quote! {
        #function
        #register_test
    }
    .into_token_stream()
    .into()
}

fn parse_marked_test_function(attributes: &AttributeArgs, function: &syn::ItemFn) -> TestFunction {
    let macro_parameters = get_test_macro_parameters(attributes);

    let function_parameters = get_test_function_parameters(&function.sig.inputs);

    TestFunction {
        name: function.sig.ident.clone(),
        function_parameters,
        macro_parameters,
    }
}

fn get_test_macro_parameters(attributes: &syn::AttributeArgs) -> MacroParameters {
    let mut priority = None;
    let mut cleanup = true;
    for attribute in attributes {
        if let NestedMeta::Meta(Meta::NameValue(nv)) = attribute {
            if nv.path.is_ident("priority") {
                match &nv.lit {
                    Lit::Int(lit_int) => {
                        priority = Some(lit_int.clone());
                    }
                    _ => panic!("'priority' should have an integer value"),
                }
            } else if nv.path.is_ident("cleanup") {
                match &nv.lit {
                    Lit::Bool(lit_bool) => {
                        cleanup = lit_bool.value();
                    }
                    _ => panic!("'cleanup' should have a bool value"),
                }
            }
        }
    }

    MacroParameters { priority, cleanup }
}

fn create_test(test_function: TestFunction) -> proc_macro2::TokenStream {
    let test_function_priority = match test_function.macro_parameters.priority {
        Some(priority) => quote! {Some(#priority)},
        None => quote! {None},
    };
    let should_cleanup = test_function.macro_parameters.cleanup;

    let func_name = test_function.name;
    let function_mullvad_version = test_function.function_parameters.mullvad_client.version();
    let wrapper_closure = match test_function.function_parameters.mullvad_client {
        MullvadClient::New {
            mullvad_client_type,
            ..
        } => {
            let mullvad_client_type = *mullvad_client_type;
            quote! {
                |rpc: test_rpc::ServiceClient,
                mullvad_client: Box<dyn std::any::Any + Send>,|
                {
                    use std::any::Any;
                    let mut mullvad_client = mullvad_client.downcast::<#mullvad_client_type>().expect("invalid mullvad client");
                    Box::pin(async move {
                        // If default settings are not retrieved, retrieve them
                        let default_settings = crate::tests::get_default_settings(&mut mullvad_client).await;
                        let result = #func_name(rpc, *mullvad_client.clone()).await;
                        if #should_cleanup {
                            crate::tests::cleanup_after_test(default_settings, Some(*mullvad_client)).await?;
                        }
                        result
                    })
                }
            }
        }
        MullvadClient::None { .. } => {
            quote! {
                |rpc: test_rpc::ServiceClient,
                mullvad_client: Box<dyn std::any::Any + Send>,| {
                    Box::pin(async move {
                        #func_name(rpc).await
                    })
                }
            }
        }
    };

    quote! {
        inventory::submit!(crate::tests::test_metadata::TestMetadata {
            name: stringify!(#func_name),
            command: stringify!(#func_name),
            mullvad_client_version: #function_mullvad_version,
            func: Box::new(#wrapper_closure),
            priority: #test_function_priority,
        });
    }
}

struct TestFunction {
    name: syn::Ident,
    function_parameters: FunctionParameters,
    macro_parameters: MacroParameters,
}

struct MacroParameters {
    priority: Option<syn::LitInt>,
    cleanup: bool,
}

enum MullvadClient {
    None {
        mullvad_client_version: proc_macro2::TokenStream,
    },
    New {
        mullvad_client_type: Box<syn::Type>,
        mullvad_client_version: proc_macro2::TokenStream,
    },
}

impl MullvadClient {
    fn version(&self) -> proc_macro2::TokenStream {
        match self {
            MullvadClient::None {
                mullvad_client_version,
            } => mullvad_client_version.clone(),
            MullvadClient::New {
                mullvad_client_version,
                ..
            } => mullvad_client_version.clone(),
        }
    }
}

struct FunctionParameters {
    mullvad_client: MullvadClient,
}

fn get_test_function_parameters(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>,
) -> FunctionParameters {
    if inputs.len() > 1 {
        match inputs[1].clone() {
            syn::FnArg::Typed(pat_type) => {
                let mullvad_client = match &*pat_type.ty {
                    syn::Type::Path(syn::TypePath { path, .. }) => {
                        match path.segments[0].ident.to_string().as_str() {
                            "mullvad_management_interface" | "ManagementServiceClient" => {
                                let mullvad_client_version =
                                    quote! { test_rpc::mullvad_daemon::MullvadClientVersion::New };
                                MullvadClient::New {
                                    mullvad_client_type: pat_type.ty,
                                    mullvad_client_version,
                                }
                            }
                            _ => panic!("cannot infer mullvad client type"),
                        }
                    }
                    _ => panic!("unexpected 'mullvad_client' type"),
                };
                FunctionParameters { mullvad_client }
            }
            syn::FnArg::Receiver(_) => panic!("unexpected 'mullvad_client' arg"),
        }
    } else {
        FunctionParameters {
            mullvad_client: MullvadClient::None {
                mullvad_client_version: quote! { test_rpc::mullvad_daemon::MullvadClientVersion::None },
            },
        }
    }
}
