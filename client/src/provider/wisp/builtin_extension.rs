use std::collections::HashMap;

use wasm_bindgen::prelude::wasm_bindgen;
use wisp_mux::extensions::{
	cert::{
		CertAuthProtocolExtension as InnerCertAuthProtocolExtension,
		CertAuthProtocolExtensionBuilder as InnerCertAuthProtocolExtensionBuilder,
	},
	motd::{
		MotdProtocolExtension as InnerMotdProtocolExtension,
		MotdProtocolExtensionBuilder as InnerMotdProtocolExtensionBuilder,
	},
	password::{
		PasswordProtocolExtension as InnerPasswordProtocolExtension,
		PasswordProtocolExtensionBuilder as InnerPasswordProtocolExtensionBuilder,
	},
	udp::{
		UdpProtocolExtension as InnerUdpProtocolExtension,
		UdpProtocolExtensionBuilder as InnerUdpProtocolExtensionBuilder,
	},
};

use crate::{EpoxyError, refstruct};

#[wasm_bindgen]
pub struct MotdProtocolExtensionBuilder(
	#[wasm_bindgen(skip)] pub InnerMotdProtocolExtensionBuilder,
);

#[wasm_bindgen]
impl MotdProtocolExtensionBuilder {
	pub fn new_client() -> Self {
		Self(InnerMotdProtocolExtensionBuilder::new_client())
	}

	pub fn new_server(motd: String) -> Self {
		Self(InnerMotdProtocolExtensionBuilder::new_server(motd))
	}
}

refstruct!(
	InnerMotdProtocolExtensionBuilder,
	MotdProtocolExtensionBuilderRef
);
#[wasm_bindgen]
impl MotdProtocolExtensionBuilderRef {
	pub fn motd(&mut self) -> Result<Option<String>, EpoxyError> {
		Ok(match self.inner()? {
			InnerMotdProtocolExtensionBuilder::Server(motd) => Some(motd.clone()),
			InnerMotdProtocolExtensionBuilder::Client => None,
		})
	}

	pub fn is_client(&mut self) -> Result<bool, EpoxyError> {
		Ok(matches!(
			self.inner()?,
			InnerMotdProtocolExtensionBuilder::Client
		))
	}
}

refstruct!(InnerMotdProtocolExtension, MotdProtocolExtension);
#[wasm_bindgen]
impl MotdProtocolExtension {
	pub fn motd(&mut self) -> Result<String, EpoxyError> {
		Ok(self.inner()?.motd.clone())
	}
}

#[wasm_bindgen]
pub struct UdpProtocolExtensionBuilder(#[wasm_bindgen(skip)] pub InnerUdpProtocolExtensionBuilder);

#[wasm_bindgen]
impl UdpProtocolExtensionBuilder {
	pub fn new_client() -> Self {
		Self(InnerUdpProtocolExtensionBuilder)
	}

	pub fn new_server() -> Self {
		Self(InnerUdpProtocolExtensionBuilder)
	}
}

refstruct!(
	InnerUdpProtocolExtensionBuilder,
	UdpProtocolExtensionBuilderRef
);

refstruct!(InnerUdpProtocolExtension, UdpProtocolExtension);

#[wasm_bindgen]
pub struct PasswordProtocolExtensionBuilder(
	#[wasm_bindgen(skip)] pub InnerPasswordProtocolExtensionBuilder,
);

#[wasm_bindgen]
impl PasswordProtocolExtensionBuilder {
	pub fn new_client(user: Option<String>, password: Option<String>) -> Self {
		let creds = match (user, password) {
			(Some(user), Some(password)) => Some((user, password)),
			_ => None,
		};

		Self(InnerPasswordProtocolExtensionBuilder::new_client(creds))
	}

	pub fn new_server(required: bool) -> Self {
		Self(InnerPasswordProtocolExtensionBuilder::new_server(
			HashMap::new(),
			required,
		))
	}
}

refstruct!(
	InnerPasswordProtocolExtensionBuilder,
	PasswordProtocolExtensionBuilderRef
);
#[wasm_bindgen]
impl PasswordProtocolExtensionBuilderRef {
	pub fn is_required(&mut self) -> Result<Option<bool>, EpoxyError> {
		Ok(self.inner()?.is_required())
	}
}

refstruct!(InnerPasswordProtocolExtension, PasswordProtocolExtension);
#[wasm_bindgen]
impl PasswordProtocolExtension {
	pub fn required(&mut self) -> Result<Option<bool>, EpoxyError> {
		Ok(match self.inner()? {
			InnerPasswordProtocolExtension::ServerBeforeClientInfo { required } => Some(*required),
			InnerPasswordProtocolExtension::ServerAfterClientInfo { .. }
			| InnerPasswordProtocolExtension::ClientBeforeServerInfo
			| InnerPasswordProtocolExtension::ClientAfterServerInfo { .. } => None,
		})
	}

	pub fn user(&mut self) -> Result<Option<String>, EpoxyError> {
		Ok(match self.inner()? {
			InnerPasswordProtocolExtension::ClientAfterServerInfo { user, .. } => {
				Some(user.clone())
			}
			_ => None,
		})
	}

	pub fn chosen_user(&mut self) -> Result<Option<String>, EpoxyError> {
		Ok(match self.inner()? {
			InnerPasswordProtocolExtension::ServerAfterClientInfo { chosen_user, .. } => {
				Some(chosen_user.clone())
			}
			_ => None,
		})
	}
}

#[wasm_bindgen]
pub struct CertAuthProtocolExtensionBuilder(
	#[wasm_bindgen(skip)] pub InnerCertAuthProtocolExtensionBuilder,
);

#[wasm_bindgen]
impl CertAuthProtocolExtensionBuilder {
	pub fn new_client() -> Self {
		Self(InnerCertAuthProtocolExtensionBuilder::new_client(None))
	}

	pub fn new_server(required: bool) -> Self {
		Self(InnerCertAuthProtocolExtensionBuilder::new_server(
			Vec::new(),
			required,
		))
	}
}

refstruct!(
	InnerCertAuthProtocolExtensionBuilder,
	CertAuthProtocolExtensionBuilderRef
);
#[wasm_bindgen]
impl CertAuthProtocolExtensionBuilderRef {
	pub fn is_required(&mut self) -> Result<Option<bool>, EpoxyError> {
		Ok(self.inner()?.is_required())
	}
}

refstruct!(InnerCertAuthProtocolExtension, CertAuthProtocolExtension);
#[wasm_bindgen]
impl CertAuthProtocolExtension {
	pub fn required(&mut self) -> Result<Option<bool>, EpoxyError> {
		Ok(match self.inner()? {
			InnerCertAuthProtocolExtension::Server { required, .. } => Some(*required),
			_ => None,
		})
	}
}
