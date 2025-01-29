//! Certificate authentication protocol extension.
//!

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use ed25519::{
	signature::{Signer, Verifier},
	Signature,
};

use crate::{Role, WispError};

use super::{AnyProtocolExtension, ProtocolExtension, ProtocolExtensionBuilder};

/// Certificate authentication protocol extension error.
#[derive(Debug)]
pub enum CertAuthError {
	/// ED25519 error
	Ed25519(ed25519::Error),
	/// Getrandom error
	Getrandom(getrandom::Error),
}

impl std::fmt::Display for CertAuthError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Ed25519(x) => write!(f, "ED25519: {x:?}"),
			Self::Getrandom(x) => write!(f, "getrandom: {x:?}"),
		}
	}
}
impl std::error::Error for CertAuthError {}

impl From<ed25519::Error> for CertAuthError {
	fn from(value: ed25519::Error) -> Self {
		CertAuthError::Ed25519(value)
	}
}
impl From<getrandom::Error> for CertAuthError {
	fn from(value: getrandom::Error) -> Self {
		CertAuthError::Getrandom(value)
	}
}
impl From<CertAuthError> for WispError {
	fn from(value: CertAuthError) -> Self {
		WispError::ExtensionImplError(Box::new(value))
	}
}

bitflags::bitflags! {
	/// Supported certificate types for certificate authentication.
	#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
	pub struct SupportedCertificateTypes: u8 {
		/// ED25519 certificate.
		const Ed25519 = 0b0000_0001;
	}
}

/// Verification key.
#[derive(Clone)]
pub struct VerifyKey {
	/// Certificate type of the keypair.
	pub cert_type: SupportedCertificateTypes,
	/// SHA-256 hash of the public key.
	pub hash: [u8; 32],
	/// Verifier.
	pub verifier: Arc<dyn Verifier<Signature> + Sync + Send>,
}

impl VerifyKey {
	/// Create a new ED25519 verification key.
	pub fn new_ed25519(
		verifier: Arc<dyn Verifier<Signature> + Sync + Send>,
		hash: [u8; 32],
	) -> Self {
		Self {
			cert_type: SupportedCertificateTypes::Ed25519,
			hash,
			verifier,
		}
	}
}

/// Signing key.
#[derive(Clone)]
pub struct SigningKey {
	/// Certificate type of the keypair.
	pub cert_type: SupportedCertificateTypes,
	/// SHA-256 hash of the public key.
	pub hash: [u8; 32],
	/// Signer.
	pub signer: Arc<dyn Signer<Signature> + Sync + Send>,
}
impl SigningKey {
	/// Create a new ED25519 signing key.
	pub fn new_ed25519(signer: Arc<dyn Signer<Signature> + Sync + Send>, hash: [u8; 32]) -> Self {
		Self {
			cert_type: SupportedCertificateTypes::Ed25519,
			hash,
			signer,
		}
	}
}

/// Certificate authentication protocol extension.
#[derive(Debug, Clone)]
pub enum CertAuthProtocolExtension {
	/// Server variant of certificate authentication protocol extension.
	Server {
		/// Supported certificate types on the server.
		cert_types: SupportedCertificateTypes,
		/// Random challenge for the client.
		challenge: Bytes,
		/// Whether the server requires this authentication method.
		required: bool,
	},
	/// Client variant of certificate authentication protocol extension.
	Client {
		/// Chosen certificate type.
		cert_type: SupportedCertificateTypes,
		/// SHA-256 hash of public key.
		hash: [u8; 32],
		/// Signature of challenge.
		signature: Bytes,
	},
	/// Marker that client has successfully recieved the challenge.
	ClientRecieved,
	/// Marker that server has successfully verified the client.
	ServerVerified,
}

impl CertAuthProtocolExtension {
	/// ID of certificate authentication protocol extension.
	pub const ID: u8 = 0x03;
}

#[async_trait]
impl ProtocolExtension for CertAuthProtocolExtension {
	fn get_id(&self) -> u8 {
		Self::ID
	}

	fn encode(&self) -> Bytes {
		match self {
			Self::Server {
				cert_types,
				challenge,
				required,
			} => {
				let mut out = BytesMut::with_capacity(2 + challenge.len());
				out.put_u8(u8::from(*required));
				out.put_u8(cert_types.bits());
				out.extend_from_slice(challenge);
				out.freeze()
			}
			Self::Client {
				cert_type,
				hash,
				signature,
			} => {
				let mut out = BytesMut::with_capacity(1 + signature.len());
				out.put_u8(cert_type.bits());
				out.extend_from_slice(hash);
				out.extend_from_slice(signature);
				out.freeze()
			}
			Self::ServerVerified | Self::ClientRecieved => Bytes::new(),
		}
	}

	fn box_clone(&self) -> Box<dyn ProtocolExtension + Sync + Send> {
		Box::new(self.clone())
	}
}

/// Certificate authentication protocol extension builder.
pub enum CertAuthProtocolExtensionBuilder {
	/// Server variant of certificate authentication protocol extension before the challenge has
	/// been sent.
	ServerBeforeChallenge {
		/// Keypair verifiers.
		verifiers: Vec<VerifyKey>,
		/// Whether the server requires this authentication method.
		required: bool,
	},
	/// Server variant of certificate authentication protocol extension after the challenge has
	/// been sent.
	ServerAfterChallenge {
		/// Keypair verifiers.
		verifiers: Vec<VerifyKey>,
		/// Challenge to verify against.
		challenge: Bytes,
		/// Whether the server requires this authentication method.
		required: bool,
	},
	/// Client variant of certificate authentication protocol extension before the challenge has
	/// been recieved.
	ClientBeforeChallenge {
		/// Keypair signer.
		signer: Option<SigningKey>,
	},
	/// Client variant of certificate authentication protocol extension after the challenge has
	/// been recieved.
	ClientAfterChallenge {
		/// Keypair signer.
		signer: Option<SigningKey>,
		/// Supported certificate types recieved from the server.
		cert_types: SupportedCertificateTypes,
		/// Challenge recieved from the server.
		challenge: Bytes,
		/// Whether the server requires this authentication method.
		required: bool,
	},
}

impl CertAuthProtocolExtensionBuilder {
	/// Create a new server variant of the certificate authentication protocol extension.
	pub fn new_server(verifiers: Vec<VerifyKey>, required: bool) -> Self {
		Self::ServerBeforeChallenge {
			verifiers,
			required,
		}
	}

	/// Create a new client variant of the certificate authentication protocol extension.
	pub fn new_client(signer: Option<SigningKey>) -> Self {
		Self::ClientBeforeChallenge { signer }
	}

	/// Get whether this authentication method is required. Could return None if the server has not
	/// sent the certificate authentication protocol extension.
	pub fn is_required(&self) -> Option<bool> {
		match self {
			Self::ServerBeforeChallenge { required, .. }
			| Self::ServerAfterChallenge { required, .. }
			| Self::ClientAfterChallenge { required, .. } => Some(*required),
			Self::ClientBeforeChallenge { .. } => None,
		}
	}

	/// Set the credentials sent to the server, if this is a client variant.
	pub fn set_signing_key(&mut self, key: SigningKey) {
		match self {
			Self::ClientBeforeChallenge { signer } | Self::ClientAfterChallenge { signer, .. } => {
				*signer = Some(key);
			}
			Self::ServerBeforeChallenge { .. } | Self::ServerAfterChallenge { .. } => {}
		}
	}
}

#[async_trait]
impl ProtocolExtensionBuilder for CertAuthProtocolExtensionBuilder {
	fn get_id(&self) -> u8 {
		CertAuthProtocolExtension::ID
	}

	// client: 1
	// server: 2
	fn build_from_bytes(
		&mut self,
		mut bytes: Bytes,
		_: Role,
	) -> Result<AnyProtocolExtension, WispError> {
		match self {
			Self::ServerAfterChallenge {
				verifiers,
				challenge,
				..
			} => {
				// validate and parse response
				let cert_type = SupportedCertificateTypes::from_bits(bytes.get_u8())
					.ok_or(WispError::CertAuthExtensionSigInvalid)?;
				let hash = bytes.split_to(32);
				let sig = Signature::from_slice(&bytes).map_err(CertAuthError::from)?;
				let is_valid = verifiers
					.iter()
					.filter(|x| x.cert_type == cert_type && x.hash == *hash)
					.any(|x| x.verifier.verify(challenge, &sig).is_ok());

				if is_valid {
					Ok(CertAuthProtocolExtension::ServerVerified.into())
				} else {
					Err(WispError::CertAuthExtensionSigInvalid)
				}
			}
			Self::ClientBeforeChallenge { signer } => {
				let required = bytes.get_u8() != 0;
				// sign challenge
				let cert_types = SupportedCertificateTypes::from_bits(bytes.get_u8())
					.ok_or(WispError::CertAuthExtensionCertTypeInvalid)?;

				*self = Self::ClientAfterChallenge {
					signer: signer.clone(),
					cert_types,
					challenge: bytes,
					required,
				};

				Ok(CertAuthProtocolExtension::ClientRecieved.into())
			}

			// client has already recieved a challenge or
			// server should have already sent the challenge before recieving a response to parse
			Self::ClientAfterChallenge { .. } | Self::ServerBeforeChallenge { .. } => {
				Err(WispError::ExtensionImplNotSupported)
			}
		}
	}

	// client: 2
	// server: 1
	fn build_to_extension(&mut self, _: Role) -> Result<AnyProtocolExtension, WispError> {
		match self {
			Self::ServerBeforeChallenge {
				verifiers,
				required,
			} => {
				let mut challenge = [0u8; 64];
				getrandom::getrandom(&mut challenge).map_err(CertAuthError::from)?;
				let challenge = Bytes::from(challenge.to_vec());

				let required = *required;

				*self = Self::ServerAfterChallenge {
					verifiers: verifiers.clone(),
					challenge: challenge.clone(),
					required,
				};

				Ok(CertAuthProtocolExtension::Server {
					cert_types: SupportedCertificateTypes::Ed25519,
					challenge,
					required,
				}
				.into())
			}
			Self::ClientAfterChallenge {
				signer,
				challenge,
				cert_types,
				..
			} => {
				let signer = signer.as_ref().ok_or(WispError::CertAuthExtensionNoKey)?;
				if !cert_types.iter().any(|x| x == signer.cert_type) {
					return Err(WispError::CertAuthExtensionCertTypeInvalid);
				}

				let signature: Bytes = signer
					.signer
					.try_sign(challenge)
					.map_err(CertAuthError::from)?
					.to_vec()
					.into();

				Ok(CertAuthProtocolExtension::Client {
					cert_type: signer.cert_type,
					hash: signer.hash,
					signature: signature.clone(),
				}
				.into())
			}

			// server has already sent a challenge or
			// client needs to recieve a challenge
			Self::ClientBeforeChallenge { .. } | Self::ServerAfterChallenge { .. } => {
				Err(WispError::ExtensionImplNotSupported)
			}
		}
	}
}
