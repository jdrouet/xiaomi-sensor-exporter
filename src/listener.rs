use bluer::{
    AdapterEvent, Address, Device, DeviceEvent, DeviceProperty, DiscoveryFilter, DiscoveryTransport,
};
use futures::{pin_mut, stream::SelectAll, StreamExt};

const SERVICE_ID: u128 = 488837762788578050050668711589115;

pub struct Listener {
    #[allow(dead_code)]
    session: bluer::Session,
    adapter: bluer::Adapter,
    service_id: bluer::Uuid,
}

impl Listener {
    pub async fn new() -> bluer::Result<Self> {
        let session = bluer::Session::new().await?;
        let adapter = session.default_adapter().await?;
        let service_id = bluer::Uuid::from_u128(SERVICE_ID);

        Ok(Listener {
            session,
            adapter,
            service_id,
        })
    }

    async fn handle_device(&self, address: Address, device: &Device) -> bluer::Result<()> {
        if let Some(mut service_data) = device.service_data().await? {
            if let Some(value) = service_data.remove(&self.service_id) {
                let name = device.name().await?.unwrap_or_default();

                if let Some(entry) = crate::model::Entry::build(address, name, &value) {
                    entry.trace();
                }
            }
        }

        Ok(())
    }

    async fn handle_change(&self, address: Address, property: DeviceProperty) -> bluer::Result<()> {
        if let DeviceProperty::ServiceData(mut service_data) = property {
            if let Some(value) = service_data.remove(&self.service_id) {
                let device = self.adapter.device(address)?;
                let name = device.name().await?.unwrap_or_default();

                if let Some(entry) = crate::model::Entry::build(address, name, &value) {
                    entry.trace();
                }
            }
        }

        Ok(())
    }

    pub async fn run(&mut self) -> bluer::Result<()> {
        self.adapter.set_powered(true).await?;
        self.adapter
            .set_discovery_filter(DiscoveryFilter {
                transport: DiscoveryTransport::Le,
                ..Default::default()
            })
            .await?;

        let device_events = self.adapter.discover_devices().await?;
        pin_mut!(device_events);

        let mut all_change_events = SelectAll::new();

        loop {
            tokio::select! {
                Some(device_event) = device_events.next() => {
                    if let AdapterEvent::DeviceAdded(addr) = device_event {
                        let device = self.adapter.device(addr)?;

                        if let Err(error) = self.handle_device(addr, &device).await {
                            tracing::warn!("unable to read device {addr:?}: {error:?}");
                        } else {
                            let change_events = device.events().await?.map(move |evt| (addr, evt));
                            all_change_events.push(change_events);
                        }
                    }
                }
                Some((addr, DeviceEvent::PropertyChanged(change))) = all_change_events.next() => {
                    if let Err(error) = self.handle_change(addr, change).await {
                        tracing::warn!("unable to handle change {addr:?}: {error:?}");
                    }
                }
                else => break
            }
        }

        Ok(())
    }
}
