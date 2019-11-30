use crate::{BeaconNode, ValidatorClient};
use futures::{Future, IntoFuture};
use node_test_rig::{
    environment::RuntimeContext, ClientConfig, LocalValidatorClient, RemoteBeaconNode,
    ValidatorConfig,
};
use parking_lot::RwLock;
use std::ops::Deref;
use std::sync::Arc;
use types::EthSpec;

pub struct Inner<E: EthSpec> {
    context: RuntimeContext<E>,
    beacon_nodes: RwLock<Vec<BeaconNode<E>>>,
    validator_clients: RwLock<Vec<ValidatorClient<E>>>,
}

pub struct LocalNetwork<E: EthSpec> {
    inner: Arc<Inner<E>>,
}

impl<E: EthSpec> Clone for LocalNetwork<E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<E: EthSpec> Deref for LocalNetwork<E> {
    type Target = Inner<E>;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<E: EthSpec> LocalNetwork<E> {
    /// Creates a new network with a single `BeaconNode`.
    pub fn new(context: RuntimeContext<E>, beacon_config: ClientConfig) -> Result<Self, String> {
        let beacon_nodes = vec![BeaconNode::production(
            context.service_context("boot_node".into()),
            beacon_config,
        )];

        Ok(Self {
            inner: Arc::new(Inner {
                context,
                beacon_nodes: RwLock::new(beacon_nodes),
                validator_clients: RwLock::new(vec![]),
            }),
        })
    }

    pub fn add_beacon_node(&self, mut beacon_config: ClientConfig) -> Result<(), String> {
        self.beacon_nodes
            .read()
            .first()
            .map(|boot_node| {
                beacon_config.network.boot_nodes.push(
                    boot_node
                        .client
                        .enr()
                        .expect("bootnode must have a network"),
                );
            })
            .ok_or_else(|| "No boot node".to_string())?;

        let index = self.beacon_nodes.read().len();

        let beacon_node = BeaconNode::production(
            self.context.service_context(format!("node_{}", index)),
            beacon_config,
        );

        self.beacon_nodes.write().push(beacon_node);

        Ok(())
    }

    pub fn add_validator_client(
        &self,
        mut validator_config: ValidatorConfig,
        beacon_node: usize,
        keypair_indices: Vec<usize>,
    ) -> impl Future<Item = (), Error = String> {
        let index = self.validator_clients.read().len();
        let context = self.context.service_context(format!("validator_{}", index));
        let self_1 = self.clone();

        self.beacon_nodes
            .read()
            .get(beacon_node)
            .map(move |beacon_node| {
                let socket_addr = beacon_node
                    .client
                    .http_listen_addr()
                    .expect("Must have http started");

                validator_config.http_server =
                    format!("http://{}:{}", socket_addr.ip(), socket_addr.port());

                validator_config
            })
            .ok_or_else(|| format!("No beacon node for index {}", beacon_node))
            .into_future()
            .and_then(move |validator_config| {
                LocalValidatorClient::production_with_insecure_keypairs(
                    context,
                    validator_config,
                    &keypair_indices,
                )
            })
            .map(move |validator_client| self_1.validator_clients.write().push(validator_client))
    }

    pub fn remote_nodes(&self) -> Result<Vec<RemoteBeaconNode<E>>, String> {
        let beacon_nodes = self.beacon_nodes.read();

        beacon_nodes
            .iter()
            .map(|beacon_node| beacon_node.remote_node())
            .collect()
    }
}