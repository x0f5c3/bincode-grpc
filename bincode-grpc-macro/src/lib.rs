use heck::{ToShoutySnakeCase, ToSnakeCase};
use proc_macro::TokenStream;
use quote::ToTokens;
use syn::__private::TokenStream2;
use syn::parse::{Parse, ParseStream};
use syn::{token, FnArg, ImplItem, ImplItemFn, Token};
use syn::{Attribute, Ident, ReturnType, Visibility};

/// // request type
/// ```
/// #[derive(Serialize, Deserialize, Default, Debug)]
/// pub struct HelloRequest {}
/// ```
///
/// // response type
/// ```
/// #[derive(Serialize, Deserialize, Default, Debug)]
/// pub struct HelloReply {}
/// ```
///
/// // user defined trait (`ident`)
/// ```
/// use bincode_grpc_macro::service;
/// #[service]
/// pub trait Greeter {
///     ...
/// }
/// ```
///
/// // generated create service function (`service_create_fn_ident`)
/// ```
/// pub fn create_greeter<S: Greeter + Send + Clone + 'static>(s: S) -> ::bincode_grpc::grpcio::Service {
///     let mut builder = ::bincode_grpc::grpcio::ServiceBuilder::new();
///     let mut instance = s;
///     builder = builder.add_unary_handler(&METHOD_GREETER_SAY_HELLO, move |ctx, req, resp| {
///         instance.say_hello(ctx, req, resp)
///     });
///     builder.build()
/// }
/// ```
struct Service {
    attrs: Vec<Attribute>,
    vis: Visibility,
    ident: Ident,
    rpcs: Vec<RpcMethod>,
}

impl Parse for Service {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let vis: Visibility = input.parse()?;
        input.parse::<token::Trait>()?;
        let ident: Ident = input.parse()?;
        let content;
        syn::braced!(content in input);
        let mut rpcs = Vec::<RpcMethod>::new();
        while !content.is_empty() {
            rpcs.push(content.parse()?);
        }

        Ok(Self {
            attrs,
            vis,
            ident,
            rpcs,
        })
    }
}

impl Service {
    fn service_create_fn_ident(&self) -> Ident {
        quote::format_ident!("create_{}", self.ident.to_string().as_str().to_snake_case())
    }

    fn client_ident(&self) -> Ident {
        quote::format_ident!("{}Client", self.ident)
    }

    fn client_struct(&self) -> TokenStream2 {
        let vis = &self.vis;
        let ident = self.client_ident();
        quote::quote! {
            #[derive(Clone)]
            #vis struct #ident {
                client: ::bincode_grpc::tonic::Client,
            }
        }
    }

    fn client_deref(&self) -> TokenStream2 {
        let ident = self.client_ident();
        quote::quote! {
            impl std::ops::Deref for #ident {
                type Target = ::bincode_grpc::tonic::Client;

                fn deref(&self) -> &Self::Target {
                    &self.client
                }
            }
        }
    }

    fn client_impl(&self) -> TokenStream2 {
        let vis = &self.vis;
        let ident = &self.ident;
        let client_methods: Vec<_> = self
            .rpcs
            .iter()
            .flat_map(|x| {
                vec![
                    x.client_method(),
                    x.client_method_opt(&ident),
                    x.client_method_async(),
                    x.client_method_async_opt(&ident),
                ]
            })
            .collect();

        let client_ident = self.client_ident();
        quote::quote! {
            impl #client_ident {
                #vis fn new(channel: ::bincode_grpc::grpcio::Channel) -> Self {
                    Self {
                        client: ::bincode_grpc::grpcio::Client::new(channel)
                    }
                }

                #( #vis #client_methods )*
            }
        }
    }

    fn method_declarations(&self) -> TokenStream2 {
        let vis = &self.vis;
        let ident = &self.ident;
        let method_declarations = self.rpcs.iter().map(|rpc| rpc.method_declaration(&ident));

        quote::quote! {
            #( #vis #method_declarations )*
        }
    }

    fn trait_service(&self) -> TokenStream2 {
        let attrs = &self.attrs;
        let vis = &self.vis;
        let ident = &self.ident;

        // let original_fns = self.rpcs.iter().map(|rpc| rpc.original_method());
        let grpc_fns = self.rpcs.iter().map(|rpc| rpc.grpc_method());

        quote::quote! {
            #( #attrs )*
            #vis trait #ident {
                // #( #original_fns )*
                #( #grpc_fns )*
            }
        }
    }

    fn create_service(&self) -> TokenStream2 {
        let vis = &self.vis;
        let ident = &self.ident;
        let fn_ident = self.service_create_fn_ident();
        let method_registrations = self.rpcs.iter().map(|rpc| {
            let declaration_ident = rpc.method_declaration_ident(ident);
            let grpc_ident = rpc.grpc_method_ident();
            quote::quote! {
                let mut instance = s.clone();
                builder = builder.add_unary_handler(&#declaration_ident, move |ctx, req, resp| {
                    instance.#grpc_ident(ctx, req, resp)
                });
            }
        });
        quote::quote! {
            #vis fn #fn_ident<S: #ident + Send + Clone + 'static>(s: S) -> ::bincode_grpc::tonic::Service {
                let mut builder = ::bincode_grpc::tonic::ServiceBuilder::new();
                #( #method_registrations )*
                builder.build()
            }
        }
    }
}

impl ToTokens for Service {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        tokens.extend(vec![
            self.method_declarations(),
            self.trait_service(),
            self.create_service(),
            self.client_struct(),
            self.client_deref(),
            self.client_impl(),
        ])
    }
}

///
/// // generated grpc method declarations (`method_declaration_ident`)
/// ```
/// pub const METHOD_GREETER_SAY_HELLO: grpcio::Method<HelloRequest, HelloReply> = grpcio::Method {
///     ty: MethodType::Unary,
///     name: "hello",
///     req_mar: Marshaller {
///         ser: grpcio::bi_ser,
///         de: grpcio::bi_de,
///     },
///     resp_mar: Marshaller {
///         ser: grpcio::bi_ser,
///         de: grpcio::bi_de,
///     },
/// };
/// ```
///
/// Original trait method declaration:
/// ```
///     fn say_hello(
///         &self,
///         arg1: HelloRequest,
///     ) -> HelloReply;
/// ```
///
/// a transformed method should be added to the trait (`grpc_method_ident`):
/// ```
///     fn say_hello_grpc(
///         &mut self,
///         ctx: ::bincode_grpc::grpcio::RpcContext,
///         req: (HelloRequest, ), // the tuple is used for the case where the original trait method has multiple arguments
///         sink: ::bincode_grpc::grpcio::UnarySink<HelloReply>,
///     );
/// ```
struct RpcMethod {
    attrs: Vec<Attribute>,
    ident: Ident,
    args: Vec<syn::PatType>,
    receiver: syn::Receiver,
    output: ReturnType,
}

impl Parse for RpcMethod {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        input.parse::<token::Fn>()?;
        let ident: Ident = input.parse()?;
        let content;
        syn::parenthesized!(content in input);
        let mut args = vec![];
        let mut receiver = None;
        for arg in content.parse_terminated(syn::FnArg::parse, Token![,])? {
            match arg {
                FnArg::Receiver(captures) => {
                    if captures.mutability.is_none() {
                        panic!("should be &mut self");
                    } else if receiver.is_some() {
                        panic!("duplicated self");
                    } else {
                        receiver = Some(captures);
                    }
                }
                FnArg::Typed(captures) => match *captures.pat {
                    syn::Pat::Ident(_) => args.push(captures),
                    _ => panic!("patterns aren't allowd in RPC args"),
                },
            }
        }
        let output: syn::ReturnType = input.parse()?;
        input.parse::<token::Semi>()?;
        Ok(Self {
            attrs,
            ident,
            args,
            receiver: receiver.unwrap(),
            output,
        })
    }
}

impl RpcMethod {
    /// `some_method` to `METHOD_SOME_METHOD`
    fn method_declaration_ident(&self, service_name: &Ident) -> Ident {
        quote::format_ident!(
            "{}_METHOD_{}",
            service_name.to_string().as_str().to_shouty_snake_case(),
            self.ident.to_string().as_str().to_shouty_snake_case()
        )
    }

    /// `method` to `method_grpc`
    fn grpc_method_ident(&self) -> Ident {
        quote::format_ident!("{}_grpc", self.ident)
    }

    /// original user defined methods
    fn original_method(&self) -> TokenStream2 {
        let attrs = &self.attrs;
        let ident = &self.ident;
        let args = &self.args;
        let receiver = &self.receiver;
        let output = &self.output;
        quote::quote! {
            #( #attrs )*
            fn #ident(#receiver, #( #args ),*) #output;
        }
    }

    fn req_type(&self) -> TokenStream2 {
        let args = &self.args;
        let all_arg_types: Vec<_> = args.iter().map(|x| &x.ty).collect();
        if all_arg_types.len() > 0 {
            quote::quote! {
                (#( #all_arg_types ),*,)
            }
        } else {
            quote::quote! {
                ()
            }
        }
    }

    fn resp_type(&self) -> TokenStream2 {
        let output = &self.output;
        let output = match output {
            ReturnType::Default => syn::Type::Verbatim(quote::quote! {()}),
            ReturnType::Type(_, t) => (**t).clone(),
        };
        quote::quote! {
            #output
        }
    }

    fn client_method(&self) -> TokenStream2 {
        let ident = &self.ident;
        let req_type = self.req_type();
        let resp_type = self.resp_type();
        let opt_method_ident = quote::format_ident!("{}_opt", ident);

        quote::quote! {
            fn #ident(&self, req: &#req_type) -> ::bincode_grpc::grpcio::Result<#resp_type> {
                self.#opt_method_ident(req, ::bincode_grpc::grpcio::CallOption::default())
            }
        }
    }

    fn client_method_opt(&self, server_name: &Ident) -> TokenStream2 {
        let ident = &self.ident;
        let req_type = self.req_type();
        let resp_type = self.resp_type();
        let opt_method_ident = quote::format_ident!("{}_opt", ident);
        let method_ident = self.method_declaration_ident(&server_name);

        quote::quote! {
            fn #opt_method_ident(&self, req: &#req_type, opt: ::bincode_grpc::grpcio::CallOption) -> ::bincode_grpc::grpcio::Result<#resp_type> {
                self.client.unary_call(&#method_ident, req, opt)
            }
        }
    }

    fn client_method_async_opt(&self, server_name: &Ident) -> TokenStream2 {
        let ident = &self.ident;
        let req_type = self.req_type();
        let resp_type = self.resp_type();
        let async_opt_method_ident = quote::format_ident!("{}_async_opt", ident);
        let method_ident = self.method_declaration_ident(&server_name);

        quote::quote! {
            fn #async_opt_method_ident(&self, req: &#req_type, opt: ::bincode_grpc::grpcio::CallOption) -> ::bincode_grpc::grpcio::Result<::grpcio::ClientUnaryReceiver<#resp_type>> {
                self.client.unary_call_async(&#method_ident, req, opt)
            }
        }
    }
    fn client_method_async(&self) -> TokenStream2 {
        let ident = &self.ident;
        let req_type = self.req_type();
        let resp_type = self.resp_type();
        let async_method_ident = quote::format_ident!("{}_async", ident);
        let async_opt_method_ident = quote::format_ident!("{}_async_opt", ident);

        quote::quote! {
            fn #async_method_ident(&self, req: &#req_type) -> ::bincode_grpc::grpcio::Result<::grpcio::ClientUnaryReceiver<#resp_type>> {
                self.#async_opt_method_ident(req, ::bincode_grpc::grpcio::CallOption::default())
            }
        }
    }

    /// transformed grpc compliant methods
    fn grpc_method(&self) -> TokenStream2 {
        let attrs = &self.attrs;
        let ident = &self.grpc_method_ident();
        let receiver = &self.receiver;
        let req_type = self.req_type();
        let resp_type = self.resp_type();

        quote::quote! {
            #( #attrs )*
            fn #ident(
                #receiver,
                ctx: ::bincode_grpc::grpcio::RpcContext,
                req: #req_type,
                sink: ::bincode_grpc::grpcio::UnarySink<#resp_type>
              );
        }
    }

    fn method_declaration(&self, service_name: &Ident) -> TokenStream2 {
        let ident = self.method_declaration_ident(&service_name);
        let req_type = self.req_type();
        let resp_type = self.resp_type();
        quote::quote! {
            const #ident: ::bincode_grpc::grpcio::Method<#req_type, #resp_type> = ::bincode_grpc::grpcio::Method {
                ty: ::bincode_grpc::grpcio::MethodType::Unary,
                name: stringify!(#ident),
                req_mar: ::bincode_grpc::grpcio::Marshaller {
                    ser: ::bincode_grpc::bi_codec::ser,
                    de: ::bincode_grpc::bi_codec::de,
                },
                resp_mar: ::bincode_grpc::grpcio::Marshaller {
                    ser: ::bincode_grpc::bi_codec::ser,
                    de: ::bincode_grpc::bi_codec::de,
                },
            };
        }
    }
}

#[proc_macro_attribute]
pub fn service(_attr: TokenStream, tokens: TokenStream) -> TokenStream {
    syn::parse_macro_input!(tokens as Service)
        .into_token_stream()
        .into()
}

/// ```
/// use bincode_grpc_macro::server;
/// struct GreeterServer;
///
/// #[server]
/// impl Greeter for GreeterServer {
///     fn say_hello(&mut self, req: HelloRequest) -> HelloReply {
///         HelloReply::default()
///     }
/// }
/// ```
///
/// The above code should generate
///
/// ```
/// impl Greeter for GreeterServer {
///     fn say_hello(&mut self, req: HelloRequest) -> HelloReply {
///         HelloReply::default()
///     }
///
///     fn say_hello_grpc(&mut self, ctx: RpcContext<'_>, req: (HelloRequest,), sink: UnarySink<HelloReply>) {
///         let mut resp = self.say_hello(req);
///         let f = sink
///             .success(resp)
///             .map_err(move |e| error!("failed to reply {:?}: {:?}", req, e))
///             .map(|_| ());
///         ctx.spawn(f)
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn server(_attr: TokenStream, tokens: TokenStream) -> TokenStream {
    let mut item = syn::parse_macro_input!(tokens as syn::ItemImpl);
    let new_methods: Vec<_> = item
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(m) => Some(m),
            _ => None,
        })
        .map(|m| {
            let method_ident = &m.sig.ident;
            let vis = &m.vis;
            let grpc_method_ident = quote::format_ident!("{}_grpc", method_ident);

            let req_type = {
                let args = &m.sig.inputs;
                let all_arg_types: Vec<_> = args.iter().filter_map(|x| match x {
                    FnArg::Receiver(_) => None,
                    FnArg::Typed(x) => Some(x.ty.clone()),
                }).collect();
                if all_arg_types.len() > 0 {
                    quote::quote! {
                        (#( #all_arg_types ),*,)
                    }
                } else {
                    quote::quote! {
                        ()
                    }
                }
            };

            let req_args: Vec<_> = {
                let args = &m.sig.inputs;
                let all_arg_types = args.iter().filter_map(|x| match x {
                    FnArg::Receiver(_) => None,
                    FnArg::Typed(x) => Some(x.pat.clone()),
                });
                all_arg_types
            }.collect();

            let req_args2 = req_args.clone();

            let resp_type = {
                let output = &m.sig.output;
                let output = match output {
                    ReturnType::Default => syn::Type::Verbatim(quote::quote! {()}),
                    ReturnType::Type(_, t) => (**t).clone(),
                };
                quote::quote! {
                    #output
                }
            };

            if req_args.len() > 0 {
                quote::quote! {
                    #vis fn #grpc_method_ident(&mut self, ctx: ::bincode_grpc::grpcio::RpcContext, req: #req_type, sink: ::bincode_grpc::grpcio::UnarySink<#resp_type>) {
                         let (#( #req_args, )*) = req;
                         let mut resp = self.#method_ident(#( #req_args2, )* );
                         let f = sink
                             .success(resp)
                             .map_err(move |e| ::bincode_grpc::tracing::error!("failed to reply {:?}", e))
                             .map(|_| ());
                         ctx.spawn(f)
                    }
                }
            } else {
                quote::quote! {
                    #vis fn #grpc_method_ident(&mut self, ctx: ::bincode_grpc::grpcio::RpcContext, _req: #req_type, sink: ::bincode_grpc::grpcio::UnarySink<#resp_type>) {
                         let mut resp = self.#method_ident();
                         let f = sink
                             .success(resp)
                             .map_err(move |e| ::bincode_grpc::tracing::error!("failed to reply {:?}", e))
                             .map(|_| ());
                         ctx.spawn(f)
                    }
                }
            }
        })
        .collect();

    let original_items = std::mem::replace(&mut item.items, vec![]);

    let impl_ident = item.self_ty.clone();
    let original_impl = quote::quote! {
        impl #impl_ident {
            #( #original_items )*
        }
    };

    for method in new_methods {
        let method: ImplItemFn =
            syn::parse(method.into_token_stream().into()).expect("cannot parse method");
        item.items.push(syn::ImplItem::Fn(method));
    }

    let new_item = item.into_token_stream();

    (quote::quote! {
        #original_impl
        #new_item
    })
    .into()
}
