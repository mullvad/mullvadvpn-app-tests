use proc_macro::TokenStream;
use quote::{ToTokens, quote, format_ident};
use convert_case::{Case, Casing};

#[proc_macro_attribute]
pub fn test_module(_: TokenStream, code: TokenStream) -> TokenStream {
    let ast: syn::ItemMod = syn::parse(code).unwrap();

    let (mut ast, marked_functions) = get_function_attributes(ast);

    let struct_and_impl = create_test_struct_and_impl(&ast, marked_functions);

    ast.content.as_mut().expect("Module must have a body").1.push(syn::Item::Verbatim(struct_and_impl));
    ast.into_token_stream().into()
}

fn create_test_struct_and_impl(ast: &syn::ItemMod, marked_functions: MarkedFunctions) -> proc_macro2::TokenStream {
    let module_name = &ast.ident;
    
    let mut test_name_wrappers = vec![];
    let marked_funcs = &marked_functions.marked_funcs;
    for marked_func in marked_funcs {
        test_name_wrappers.push(format_ident!("{}_wrapper", marked_func));
    }
    let struct_name = format_ident!("{}", module_name.to_string().to_case(Case::Pascal));

    let tokens = quote! {
        pub struct #struct_name {
            /// Vec of (test name, test command, test function wrapper)
            pub tests: Vec<(&'static str, &'static str, Box<dyn Fn(ServiceClient) -> BoxFuture<'static, Result<(), Error>>>)>
        }

        impl #struct_name {
            pub fn new() -> Self {
                Self {
                    tests: vec![
                        #((stringify!(#marked_funcs), stringify!(#marked_funcs), Box::new(Self::#test_name_wrappers))),*
                    ],
                }
            }

            #(fn #test_name_wrappers(rpc: ServiceClient) -> BoxFuture<'static, Result<(), Error>> {
                Box::pin(#marked_funcs(rpc))
            })*
    
        }
    };
    tokens
}

struct MarkedFunctions {
    marked_funcs: Vec<syn::Ident>,
}

fn get_function_attributes(mut ast: syn::ItemMod) -> (syn::ItemMod, MarkedFunctions) {
    let marked_functions = match &mut ast.content {
        None => return (ast, MarkedFunctions { marked_funcs: vec![] }),
        Some((_, items)) => {
            let mut marked_funcs = vec![];
            for item in items {
                if let syn::Item::Fn(function) = item {
                    for i in 0..function.attrs.len() {
                        let attribute = &function.attrs[i];
                        if attribute.path.is_ident("manager_test") {
                            function.attrs.remove(i);
                            marked_funcs.push(function.sig.ident.clone());
                            break;
                        }
                    }
                }
            }
            MarkedFunctions { marked_funcs }
        }
    };

    (ast, marked_functions)
}
