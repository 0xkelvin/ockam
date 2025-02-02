use ockam_core::compat::sync::Arc;
use ockam_core::Result;
use ockam_core::{Address, Route};
use ockam_node::Context;

use crate::identities::Identities;
use crate::models::Identifier;
use crate::secure_channel::handshake_worker::HandshakeWorker;
use crate::secure_channel::{
    Addresses, RemoteRoute, Role, SecureChannelListenerOptions, SecureChannelListenerWorker,
    SecureChannelOptions, SecureChannelRegistry,
};
#[cfg(feature = "storage")]
use crate::SecureChannelsBuilder;
use crate::{SecureChannel, SecureChannelListener, Vault};

/// Identity implementation
#[derive(Clone)]
pub struct SecureChannels {
    pub(crate) identities: Arc<Identities>,
    pub(crate) secure_channel_registry: SecureChannelRegistry,
}

impl SecureChannels {
    /// Constructor
    pub fn new(
        identities: Arc<Identities>,
        secure_channel_registry: SecureChannelRegistry,
    ) -> Self {
        Self {
            identities,
            secure_channel_registry,
        }
    }

    /// Constructor
    pub fn from_identities(identities: Arc<Identities>) -> Arc<Self> {
        Arc::new(Self::new(identities, SecureChannelRegistry::default()))
    }

    /// Return the identities services associated to this service
    pub fn identities(&self) -> Arc<Identities> {
        self.identities.clone()
    }

    /// Vault
    pub fn vault(&self) -> Vault {
        self.identities.vault()
    }

    /// Return the secure channel registry
    pub fn secure_channel_registry(&self) -> SecureChannelRegistry {
        self.secure_channel_registry.clone()
    }

    /// Create a builder for secure channels
    #[cfg(feature = "storage")]
    pub async fn builder() -> Result<SecureChannelsBuilder> {
        Ok(SecureChannelsBuilder {
            identities_builder: Identities::builder().await?,
            registry: SecureChannelRegistry::new(),
        })
    }
}

impl SecureChannels {
    /// Spawns a SecureChannel listener at given `Address` with given [`SecureChannelListenerOptions`]
    pub async fn create_secure_channel_listener(
        &self,
        ctx: &Context,
        identifier: &Identifier,
        address: impl Into<Address>,
        options: impl Into<SecureChannelListenerOptions>,
    ) -> Result<SecureChannelListener> {
        let address = address.into();
        let options = options.into();
        let flow_control_id = options.flow_control_id.clone();

        SecureChannelListenerWorker::create(
            ctx,
            Arc::new(self.clone()),
            identifier,
            address.clone(),
            options,
        )
        .await?;

        Ok(SecureChannelListener::new(address, flow_control_id))
    }

    /// Initiate a SecureChannel using `Route` to the SecureChannel listener and [`SecureChannelOptions`]
    pub async fn create_secure_channel(
        &self,
        ctx: &Context,
        identifier: &Identifier,
        route: impl Into<Route>,
        options: impl Into<SecureChannelOptions>,
    ) -> Result<SecureChannel> {
        let addresses = Addresses::generate(Role::Initiator);
        let options = options.into();
        let flow_control_id = options.flow_control_id.clone();

        let route = route.into();
        let next = route.next()?;
        options.setup_flow_control(ctx.flow_controls(), &addresses, next);
        let decryptor_outgoing_access_control =
            options.create_decryptor_outgoing_access_control(ctx.flow_controls());

        // TODO: Allow manual PurposeKey management
        let purpose_key = self
            .identities
            .purpose_keys()
            .purpose_keys_creation()
            .get_or_create_secure_channel_purpose_key(identifier)
            .await?;

        let credential_retriever = match &options.credential_retriever_creator {
            Some(credential_retriever_creator) => {
                let credential_retriever = credential_retriever_creator.create(identifier).await?;
                credential_retriever.initialize().await?;
                Some(credential_retriever)
            }
            None => None,
        };

        let encryptor_remote_route = RemoteRoute::create();
        HandshakeWorker::create(
            ctx,
            Arc::new(self.clone()),
            addresses.clone(),
            identifier.clone(),
            purpose_key,
            options.trust_policy,
            decryptor_outgoing_access_control,
            credential_retriever,
            options.authority,
            Some(route),
            Some(options.timeout),
            Role::Initiator,
            encryptor_remote_route.clone(),
        )
        .await?;

        Ok(SecureChannel::new(
            ctx.flow_controls().clone(),
            encryptor_remote_route,
            addresses,
            flow_control_id,
        ))
    }

    /// Stop a SecureChannel given an encryptor address
    pub async fn stop_secure_channel(&self, ctx: &Context, channel: &Address) -> Result<()> {
        ctx.stop_worker(channel.clone()).await
    }
}
