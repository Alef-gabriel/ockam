use crate::cli_state::CliState;
use crate::identity::models::*;
use core::convert::Infallible;
use minicbor::encode::Write;
use minicbor::{Decoder, Encode};
use ockam_core::api::{Error, Id, Method, Request, Response, Status};
use ockam_core::compat::sync::Arc;
use ockam_core::vault::Signature;
use ockam_core::{Address, DenyAll, Result, Routed, Worker};
use ockam_identity::change_history::IdentityHistoryComparison;
use ockam_identity::{Identity, IdentityVault, PublicIdentity};
use ockam_node::Context;
use tracing::trace;

/// Vault Service Worker
pub struct IdentityService {
    ctx: Context,
    vault: Arc<dyn IdentityVault>,
    cli_state: CliState,
}

impl IdentityService {
    pub async fn new(
        ctx: &Context,
        vault: Arc<dyn IdentityVault>,
        cli_state: CliState,
    ) -> Result<Self> {
        Ok(Self {
            ctx: ctx
                .new_detached(
                    Address::random_tagged("IdentityService.root"),
                    DenyAll,
                    DenyAll,
                )
                .await?,
            vault,
            cli_state,
        })
    }
}

impl IdentityService {
    fn response_for_bad_request<W>(req: &Request, msg: &str, enc: W) -> Result<()>
    where
        W: Write<Error = Infallible>,
    {
        let error = Error::new(req.path()).with_message(msg);

        let error = if let Some(m) = req.method() {
            error.with_method(m)
        } else {
            error
        };

        Response::bad_request(req.id()).body(error).encode(enc)?;

        Ok(())
    }

    fn ok_response<W, B>(req: &Request, body: Option<B>, enc: W) -> Result<()>
    where
        W: Write<Error = Infallible>,
        B: Encode<()>,
    {
        Response::ok(req.id()).body(body).encode(enc)?;

        Ok(())
    }

    fn response_with_error<W>(
        req: Option<&Request>,
        status: Status,
        error: &str,
        enc: W,
    ) -> Result<()>
    where
        W: Write<Error = Infallible>,
    {
        let (path, req_id) = match req {
            None => ("", Id::fresh()),
            Some(req) => (req.path(), req.id()),
        };

        let error = Error::new(path).with_message(error);

        Response::builder(req_id, status).body(error).encode(enc)?;

        Ok(())
    }

    async fn handle_request<W>(
        &mut self,
        req: &Request<'_>,
        dec: &mut Decoder<'_>,
        enc: W,
    ) -> Result<()>
    where
        W: Write<Error = Infallible>,
    {
        trace! {
            target: "ockam_identity::service",
            id     = %req.id(),
            method = ?req.method(),
            path   = %req.path(),
            body   = %req.has_body(),
            "request"
        }

        let method = match req.method() {
            Some(m) => m,
            None => return Self::response_for_bad_request(req, "empty method", enc),
        };

        use Method::*;

        match method {
            Get => match req.path_segments::<2>().as_slice() {
                [identity_name] => {
                    let identity = self.cli_state.identities.get(identity_name)?;
                    let body = CreateResponse::new(
                        identity.config.change_history.export()?,
                        String::from(identity.config.identifier),
                    );
                    Self::ok_response(req, Some(body), enc)
                }
                _ => Self::response_for_bad_request(req, "unknown path", enc),
            },
            Post => match req.path_segments::<2>().as_slice() {
                [""] => {
                    let identity = Identity::create_arc(&self.ctx, self.vault.clone()).await?;
                    let identifier = identity.identifier();

                    let body =
                        CreateResponse::new(identity.export().await?, String::from(identifier));

                    Self::ok_response(req, Some(body), enc)
                }
                ["actions", "validate_identity_change_history"] => {
                    if !req.has_body() {
                        return Self::response_for_bad_request(req, "empty body", enc);
                    }

                    let args = dec.decode::<ValidateIdentityChangeHistoryRequest>()?;
                    let identity =
                        Identity::import_arc(&self.ctx, args.identity(), self.vault.clone())
                            .await?;

                    let body = ValidateIdentityChangeHistoryResponse::new(String::from(
                        identity.identifier(),
                    ));

                    Self::ok_response(req, Some(body), enc)
                }
                ["actions", "create_signature"] => {
                    if !req.has_body() {
                        return Self::response_for_bad_request(req, "empty body", enc);
                    }

                    let args = dec.decode::<CreateSignatureRequest>()?;
                    let identity = match args.vault_name() {
                        None => {
                            Identity::import_arc(&self.ctx, args.identity(), self.vault.clone())
                                .await?
                        }

                        Some(vault_name) => {
                            let vault = self.cli_state.vaults.get(&vault_name)?.get().await?;
                            Identity::import(&self.ctx, args.identity(), vault).await?
                        }
                    };

                    let signature = identity.create_signature(args.data(), None).await?;

                    let body = CreateSignatureResponse::new(signature.as_ref());

                    Self::ok_response(req, Some(body), enc)
                }
                ["actions", "verify_signature"] => {
                    if !req.has_body() {
                        return Self::response_for_bad_request(req, "empty body", enc);
                    }

                    let args = dec.decode::<VerifySignatureRequest>()?;
                    let peer_identity =
                        PublicIdentity::import_arc(args.signer_identity(), self.vault.clone())
                            .await?;

                    let verified = peer_identity
                        .verify_signature(
                            &Signature::new(args.signature().to_vec()),
                            args.data(),
                            None,
                            self.vault.clone(),
                        )
                        .await?;

                    let body = VerifySignatureResponse::new(verified);

                    Self::ok_response(req, Some(body), enc)
                }
                ["actions", "compare_identity_change_history"] => {
                    if !req.has_body() {
                        return Self::response_for_bad_request(req, "empty body", enc);
                    }

                    let args = dec.decode::<CompareIdentityChangeHistoryRequest>()?;

                    let current_identity =
                        PublicIdentity::import_arc(args.current_identity(), self.vault.clone())
                            .await?;

                    let body = if args.known_identity().is_empty() {
                        IdentityHistoryComparison::Newer
                    } else {
                        let known_identity =
                            PublicIdentity::import_arc(args.known_identity(), self.vault.clone())
                                .await?;

                        current_identity.compare(&known_identity)
                    };

                    Self::ok_response(req, Some(body), enc)
                }
                _ => Self::response_for_bad_request(req, "unknown path", enc),
            },
            Put | Patch | Delete => Self::response_for_bad_request(req, "unknown method", enc),
        }
    }

    async fn on_request(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let mut buf = Vec::new();

        let mut dec = Decoder::new(data);
        let req: Request = match dec.decode() {
            Ok(r) => r,
            Err(_) => {
                Self::response_with_error(
                    None,
                    Status::BadRequest,
                    "invalid Request structure",
                    &mut buf,
                )?;

                return Ok(buf);
            }
        };

        match self.handle_request(&req, &mut dec, &mut buf).await {
            Ok(_) => {}
            Err(err) => Self::response_with_error(
                Some(&req),
                Status::InternalServerError,
                &err.to_string(),
                &mut buf,
            )?,
        }

        Ok(buf)
    }
}

#[ockam_core::worker]
impl Worker for IdentityService {
    type Message = Vec<u8>;
    type Context = Context;

    async fn handle_message(
        &mut self,
        ctx: &mut Self::Context,
        msg: Routed<Self::Message>,
    ) -> Result<()> {
        let buf = self.on_request(msg.as_body()).await?;
        ctx.send(msg.return_route(), buf).await
    }
}
