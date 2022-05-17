use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(feature = "async-net")]
pub mod async_net;

#[cfg(feature = "zeromq")]
pub mod impl_zeromq;

pub trait Topic {
    type Pub: Send + Sync + Serialize;
    type Sub: Send + Sync + DeserializeOwned;

    const NAME: &'static str;

    fn serialize<S>(topic: &Self::Pub, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let data = (Self::NAME, topic);
        data.serialize(serializer)
    }

    fn deserialize<T, D>(deserializer: D) -> Result<(String, T), D::Error>
    where
        T: DeserializeOwned,
        D: Deserializer<'static>,
    {
        <(String, T)>::deserialize(deserializer)
    }
}

pub trait Service {
    type Req: Send + Sync + Serialize + DeserializeOwned + 'static;
    type Res: Send + Sync + Serialize + DeserializeOwned + 'static;

    const NAME: &'static str;
}

#[macro_export]
macro_rules! create_topic {
    ($topic_name: tt; $pub_sub: ty) => {
        create_topic!($topic_name; $pub_sub => $pub_sub);
    };

    ($topic_name: tt; $pub: ty => $sub: ty) => {
        pub struct $topic_name;

        impl crate::Topic for $topic_name {
            type Pub = $pub;
            type Sub = $sub;

            const NAME: &'static str = stringify!($topic_name);
        }
    };
}

#[macro_export]
macro_rules! create_service {
    ($service_name: tt; $req: ty => $res: ty) => {
        pub struct $service_name;

        impl crate::Service for $service_name {
            type Req = $req;
            type Res = $res;

            const NAME: &'static str = stringify!($service_name);
        }
    };
}

pub mod topic {
    use super::*;
    use blockchain_core::*;

    create_topic!(PubsubExample; i32 => i32);
    create_topic!(NotifyAddress; Address);
    create_topic!(NotifyTransfer; Transfer<Verified> => Transfer<Yet>);
    create_topic!(CreateTransaction; VerifiedTransaction => UnverifiedTransaction);
    create_topic!(NotifyBlock; VerifiedBlock => UnverifiedBlock);
    create_topic!(NotifyBlockHeight; Option<BlockHeight>);
    create_topic!(RequestUtxoByAddress; Address);
    create_topic!(RespondUtxoByAddress; Vec<Transition<Verified>> => Vec<Transition<Yet>>);
}

pub mod service {
    use super::*;
    use blockchain_core::*;

    create_service!(QueryExample; i32 => String);
    create_service!(QueryBlockByHeight; BlockHeight => UnverifiedBlock);
    create_service!(QueryUtxoByAddress; Address => Vec<Transfer<Yet>>);
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
