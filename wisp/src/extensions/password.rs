//! Password protocol extension.
//!
//! **Passwords are sent in plain text!!**
//!
//! See [the docs](https://github.com/MercuryWorkshop/wisp-protocol/blob/v2/protocol.md#0x02---password-authentication)
use std::collections::HashMap;

use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::{Role, WispError};

use super::{AnyProtocolExtension, ProtocolExtension, ProtocolExtensionBuilder};

/// ID of password protocol extension.
pub const PASSWORD_PROTOCOL_EXTENSION_ID: u8 = 0x02;

/// Password protocol extension.
///
/// **Passwords are sent in plain text!!**
///
/// See [the docs](https://github.com/MercuryWorkshop/wisp-protocol/blob/v2/protocol.md#0x02---password-authentication)
#[derive(Debug, Clone)]
pub enum PasswordProtocolExtension {
	/// Password protocol extension before the client INFO packet has been received.
	ServerBeforeClientInfo {
		/// Whether this authentication method is required.
		required: bool,
	},
	/// Password protocol extension after the client INFO packet has been received.
	ServerAfterClientInfo {
		/// The client's chosen user.
		chosen_user: String,
		/// The client's chosen password
		chosen_password: String,
	},

	/// Password protocol extension before the server INFO has been received.
	ClientBeforeServerInfo,
	/// Password protocol extension after the server INFO has been received.
	ClientAfterServerInfo {
		/// The user to send to the server.
		user: String,
		/// The password to send to the user.
		password: String,
	},
}

impl PasswordProtocolExtension {
	/// ID of password protocol extension.
	pub const ID: u8 = PASSWORD_PROTOCOL_EXTENSION_ID;
}

#[async_trait]
impl ProtocolExtension for PasswordProtocolExtension {
	fn get_id(&self) -> u8 {
		PASSWORD_PROTOCOL_EXTENSION_ID
	}

	fn encode(&self) -> Bytes {
		match self {
			Self::ServerBeforeClientInfo { required } => {
				let mut out = BytesMut::with_capacity(1);
				out.put_u8(u8::from(*required));
				out.freeze()
			}
			Self::ClientAfterServerInfo { user, password } => {
				let mut out = BytesMut::with_capacity(1 + 2 + user.len() + password.len());
				out.put_u8(user.len().try_into().unwrap());
				out.extend_from_slice(user.as_bytes());
				out.extend_from_slice(password.as_bytes());
				out.freeze()
			}

			Self::ServerAfterClientInfo { .. } | Self::ClientBeforeServerInfo => Bytes::new(),
		}
	}

	fn box_clone(&self) -> Box<dyn ProtocolExtension + Sync + Send> {
		Box::new(self.clone())
	}
}

/// Password protocol extension builder.
///
/// **Passwords are sent in plain text!!**
///
/// See [the docs](https://github.com/MercuryWorkshop/wisp-protocol/blob/v2/protocol.md#0x02---password-authentication)
pub enum PasswordProtocolExtensionBuilder {
	/// Password protocol extension builder before the client INFO has been received.
	ServerBeforeClientInfo {
		/// The user+password combinations to verify the client with.
		users: HashMap<String, String>,
		/// Whether this authentication method is required.
		required: bool,
	},
	/// Password protocol extension builder after the client INFO has been received.
	ServerAfterClientInfo {
		/// The user+password combinations to verify the client with.
		users: HashMap<String, String>,
		/// Whether this authentication method is required.
		required: bool,
	},

	/// Password protocol extension builder before the server INFO has been received.
	ClientBeforeServerInfo {
		/// The credentials to send to the server.
		creds: Option<(String, String)>,
	},
	/// Password protocol extension builder after the server INFO has been received.
	ClientAfterServerInfo {
		/// The credentials to send to the server.
		creds: Option<(String, String)>,
		/// Whether this authentication method is required.
		required: bool,
	},
}

impl PasswordProtocolExtensionBuilder {
	/// ID of password protocol extension.
	pub const ID: u8 = PASSWORD_PROTOCOL_EXTENSION_ID;

	/// Create a new server variant of the password protocol extension.
	pub fn new_server(users: HashMap<String, String>, required: bool) -> Self {
		Self::ServerBeforeClientInfo { users, required }
	}

	/// Create a new client variant of the password protocol extension with a username and password.
	pub fn new_client(creds: Option<(String, String)>) -> Self {
		Self::ClientBeforeServerInfo { creds }
	}

	/// Get whether this authentication method is required. Could return None if the server has not
	/// sent the password protocol extension.
	pub fn is_required(&self) -> Option<bool> {
		match self {
			Self::ServerBeforeClientInfo { required, .. }
			| Self::ServerAfterClientInfo { required, .. }
			| Self::ClientAfterServerInfo { required, .. } => Some(*required),
			Self::ClientBeforeServerInfo { .. } => None,
		}
	}

	/// Set the credentials sent to the server, if this is a client variant.
	pub fn set_creds(&mut self, credentials: (String, String)) {
		match self {
			Self::ClientBeforeServerInfo { creds } | Self::ClientAfterServerInfo { creds, .. } => {
				*creds = Some(credentials);
			}
			Self::ServerBeforeClientInfo { .. } | Self::ServerAfterClientInfo { .. } => {}
		}
	}
}

impl ProtocolExtensionBuilder for PasswordProtocolExtensionBuilder {
	fn get_id(&self) -> u8 {
		PASSWORD_PROTOCOL_EXTENSION_ID
	}

	fn build_to_extension(&mut self, _role: Role) -> Result<AnyProtocolExtension, WispError> {
		match self {
			Self::ServerBeforeClientInfo { users: _, required } => {
				Ok(PasswordProtocolExtension::ServerBeforeClientInfo {
					required: *required,
				}
				.into())
			}
			Self::ServerAfterClientInfo { .. } | Self::ClientBeforeServerInfo { .. } => {
				Err(WispError::ExtensionImplNotSupported)
			}
			Self::ClientAfterServerInfo { creds, .. } => {
				let (user, password) = creds.clone().ok_or(WispError::PasswordExtensionNoCreds)?;
				Ok(PasswordProtocolExtension::ClientAfterServerInfo { user, password }.into())
			}
		}
	}

	fn build_from_bytes(
		&mut self,
		mut bytes: Bytes,
		_role: Role,
	) -> Result<AnyProtocolExtension, WispError> {
		match self {
			Self::ServerBeforeClientInfo { users, required } => {
				let user_len = bytes.get_u8();

				let user = std::str::from_utf8(&bytes.split_to(user_len as usize))?.to_string();
				let password = std::str::from_utf8(&bytes)?.to_string();

				let valid = users.get(&user).is_some_and(|x| *x == password);

				*self = Self::ServerAfterClientInfo {
					users: users.clone(),
					required: *required,
				};

				if valid {
					Ok(PasswordProtocolExtension::ServerAfterClientInfo {
						chosen_user: user,
						chosen_password: password,
					}
					.into())
				} else {
					Err(WispError::PasswordExtensionCredsInvalid)
				}
			}
			Self::ClientBeforeServerInfo { creds } => {
				let required = bytes.get_u8() != 0;

				*self = Self::ClientAfterServerInfo {
					creds: creds.clone(),
					required,
				};

				Ok(PasswordProtocolExtension::ClientBeforeServerInfo.into())
			}
			Self::ClientAfterServerInfo { .. } | Self::ServerAfterClientInfo { .. } => {
				Err(WispError::ExtensionImplNotSupported)
			}
		}
	}
}
