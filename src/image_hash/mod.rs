mod phash;

use crate::error::Result;
pub use crate::image_hash::phash::ImageHash;

use async_trait::async_trait;

type HttpsClient = hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>;

#[async_trait]
trait ImageHasher {
    async fn hash(&self, url: &str) -> Result<ImageHash>;
}

#[derive(Clone, Debug)]
pub struct HyperImageHasher {
    client: HttpsClient,
}

impl HyperImageHasher {
    pub fn new(client: HttpsClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ImageHasher for HyperImageHasher {
    async fn hash(&self, url: &str) -> Result<ImageHash> {
        let resp = self.client.get(url.parse()?).await?;
        let body = hyper::body::to_bytes(resp).await?;
        Ok(crop_and_hash(&body)?)
    }
}

// Specifically for raid boss images. Remove the lower 25% of the image
// to get the boss image without the language-specific boss name.
fn crop_and_hash(bytes: &[u8]) -> Result<ImageHash> {
    use image::GenericImageView;

    let mut img = image::load_from_memory(bytes)?;
    let (w, h) = img.dimensions();
    img = img.crop(0, 0, w, h * 3 / 4);

    Ok(ImageHash::new(&img))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::model::Language;
    use std::collections::HashMap;

    struct Item {
        name: &'static str,
        level: i32,
        language: Language,
        hash: ImageHash,
    }

    // This test is slow to run, and is enabled with `--features integration`.
    // To see console output as results come in, run with `--nocapture`.
    // i.e., `cargo test --features integration -- --nocapture`
    #[cfg_attr(not(feature = "integration"), ignore)]
    #[tokio::test]
    async fn raid_equality() -> anyhow::Result<()> {
        use crate::model::Language::{English as En, Japanese as Ja};

        let conn = hyper_tls::HttpsConnector::new();
        let client = hyper::Client::builder().build::<_, hyper::Body>(conn);
        let hasher = HyperImageHasher::new(client);

        // Copied from gbf-raidfinder tests:
        // https://github.com/walfie/gbf-raidfinder/blob/master/server/src/it/scala/com/pastebin/Pj9d8jt5/ImagePHashSpec.scala
        #[rustfmt::skip]
        let bosses = vec![
            // Japanese bosses
            ("Lv30 アーフラー", 30, Ja, "https://pbs.twimg.com/media/CeSO4quUYAAy8U1.jpg"),
            ("Lv40 アーフラー", 40, Ja, "https://pbs.twimg.com/media/CeSO6aAWAAAAOGe.jpg"),
            ("Lv50 ベオウルフ", 50, Ja, "https://pbs.twimg.com/media/CeSO8gHWAAEnMGl.jpg"),
            ("Lv60 ベオウルフ", 60, Ja, "https://pbs.twimg.com/media/CeSO-QrWwAECCG1.jpg"),

            ("Lv40 ゲイザー", 40, Ja, "https://pbs.twimg.com/media/Cn7opV5VUAAfgDz.jpg"),
            ("Lv40 ヨグ＝ソトース", 40, Ja, "https://pbs.twimg.com/media/Cqlrx16VUAAU1-Z.jpg"),
            ("Lv60 グガランナ", 60, Ja, "https://pbs.twimg.com/media/Cn7o2sYVYAEP45o.jpg"),
            ("Lv60 マヒシャ", 60, Ja, "https://pbs.twimg.com/media/Cqlr4E5VMAAOGqb.jpg"),
            ("Lv75 スーペルヒガンテ", 75, Ja, "https://pbs.twimg.com/media/Cqlr_RRVYAEabiO.jpg"),
            ("Lv75 エメラルドホーン", 75, Ja, "https://pbs.twimg.com/media/Cn7o767UkAEX8-H.jpg"),

            ("Lv30 クレイゴーレム", 30, Ja, "https://pbs.twimg.com/media/Crtprh7UIAARax2.jpg"),
            ("Lv30 コロッサス", 30, Ja, "https://pbs.twimg.com/media/CT6cUf8VEAEBaEb.jpg"),
            ("Lv30 セレスト", 30, Ja, "https://pbs.twimg.com/media/CT6cmzjUcAIvSo_.jpg"),
            ("Lv30 ティアマト", 30, Ja, "https://pbs.twimg.com/media/CT6cLx4VAAAzePV.jpg"),
            ("Lv50 アドウェルサ", 50, Ja, "https://pbs.twimg.com/media/CT6cgc5UYAIhJoB.jpg"),
            ("Lv50 コロッサス", 50, Ja, "https://pbs.twimg.com/media/CT6cUf8VEAEBaEb.jpg"),
            ("Lv50 セレスト", 50, Ja, "https://pbs.twimg.com/media/CT6cmzjUcAIvSo_.jpg"),
            ("Lv50 ティアマト", 50, Ja, "https://pbs.twimg.com/media/CT6cLx4VAAAzePV.jpg"),
            ("Lv50 ティアマト・マグナ", 50, Ja, "https://pbs.twimg.com/media/CT6buTPUwAIM9VJ.jpg"),
            ("Lv50 ユグドラシル", 50, Ja, "https://pbs.twimg.com/media/CT6cbScU8AAgXRw.jpg"),
            ("Lv50 リヴァイアサン", 50, Ja, "https://pbs.twimg.com/media/CT6cXcgUEAEc0Zl.jpg"),
            ("Lv50 ヴェセラゴ", 50, Ja, "https://pbs.twimg.com/media/Crtpt5RUAAAV6OG.jpg"),
            ("Lv60 ユグドラシル・マグナ", 60, Ja, "https://pbs.twimg.com/media/CT6cDD3UkAEnP8Y.jpg"),
            ("Lv60 リヴァイアサン・マグナ", 60, Ja, "https://pbs.twimg.com/media/CT6cBK3U8AA4xdW.jpg"),
            ("Lv70 コロッサス・マグナ", 70, Ja, "https://pbs.twimg.com/media/CT6bwJTUYAA6mcV.jpg"),
            ("Lv75 シュヴァリエ・マグナ", 75, Ja, "https://pbs.twimg.com/media/CT6cEwEUcAAlwFM.jpg"),
            ("Lv75 セレスト・マグナ", 75, Ja, "https://pbs.twimg.com/media/CT6cGF4UYAAHBg5.jpg"),
            ("Lv100 Dエンジェル・オリヴィエ", 100, Ja, "https://pbs.twimg.com/media/CT6csqNVAAA_GFU.jpg"),
            ("Lv100 アテナ", 100, Ja, "https://pbs.twimg.com/media/Cg7oAJsUkAApRif.jpg"),
            ("Lv100 アポロン", 100, Ja, "https://pbs.twimg.com/media/CT6chwtUsAA0WFw.jpg"),
            ("Lv100 オーディン", 100, Ja, "https://pbs.twimg.com/media/CqwDU_jUkAQjgKq.jpg"),
            ("Lv100 ガルーダ", 100, Ja, "https://pbs.twimg.com/media/CkVbhuqVEAA7e6K.jpg"),
            ("Lv100 グラニ", 100, Ja, "https://pbs.twimg.com/media/CqwDXDkUMAAjCE5.jpg"),
            ("Lv100 コロッサス・マグナ", 100, Ja, "https://pbs.twimg.com/media/CVL2CmeUwAAElDW.jpg"),
            ("Lv100 シュヴァリエ・マグナ", 100, Ja, "https://pbs.twimg.com/media/CbTeQ1fVIAAEoqi.jpg"),
            ("Lv100 ジ・オーダー・グランデ", 100, Ja, "https://pbs.twimg.com/media/CdL4YeiUEAI0JKW.jpg"),
            ("Lv100 セレスト・マグナ", 100, Ja, "https://pbs.twimg.com/media/CbTeWuMUUAIzFZl.jpg"),
            ("Lv100 ティアマト・マグナ＝エア", 100, Ja, "https://pbs.twimg.com/media/CT6cNUBUAAETdz6.jpg"),
            ("Lv100 ナタク", 100, Ja, "https://pbs.twimg.com/media/CT6cOzzUwAESsq_.jpg"),
            ("Lv100 バアル", 100, Ja, "https://pbs.twimg.com/media/CjLxgrbUgAAFbwi.jpg"),
            ("Lv100 フラム＝グラス", 100, Ja, "https://pbs.twimg.com/media/CT_qpfCUsAA9vfF.jpg"),
            ("Lv100 プロトバハムート", 100, Ja, "https://pbs.twimg.com/media/CT6cIKmUYAAPVmD.jpg"),
            ("Lv100 マキュラ・マリウス", 100, Ja, "https://pbs.twimg.com/media/CT6cYp-UsAAy_f0.jpg"),
            ("Lv100 メドゥーサ", 100, Ja, "https://pbs.twimg.com/media/CT6ccesVEAAy_Kx.jpg"),
            ("Lv100 ユグドラシル・マグナ", 100, Ja, "https://pbs.twimg.com/media/CYBkhTmUsAAPjgu.jpg"),
            ("Lv100 リッチ", 100, Ja, "https://pbs.twimg.com/media/CqwDZKwVMAAxt0Y.jpg"),
            ("Lv100 リヴァイアサン・マグナ", 100, Ja, "https://pbs.twimg.com/media/CYBkbZSUAAA2BW-.jpg"),
            ("Lv110 ローズクイーン", 110, Ja, "https://pbs.twimg.com/media/CUnrstgUYAA4hOz.jpg"),
            ("Lv120 Dエンジェル・オリヴィエ", 120, Ja, "https://pbs.twimg.com/media/CbTeSqbUcAARoNV.jpg"),
            ("Lv120 アポロン", 120, Ja, "https://pbs.twimg.com/media/CbTeO4fUkAEmmIN.jpg"),
            ("Lv120 ギルガメッシュ", 120, Ja, "https://pbs.twimg.com/media/CqG0X1tUkAA5B8_.jpg"),
            ("Lv120 ナタク", 120, Ja, "https://pbs.twimg.com/media/CT6cQD-UcAE3nt2.jpg"),
            ("Lv120 フラム＝グラス", 120, Ja, "https://pbs.twimg.com/media/CVL2EBHUwAA8nUj.jpg"),
            ("Lv120 マキュラ・マリウス", 120, Ja, "https://pbs.twimg.com/media/CYBkd_1UEAATH9J.jpg"),
            ("Lv120 メドゥーサ", 120, Ja, "https://pbs.twimg.com/media/CYBki-CUkAQVWW_.jpg"),
            ("Lv150 プロトバハムート", 150, Ja, "https://pbs.twimg.com/media/CdL4WyxUYAIXPb8.jpg"),

            // English bosses
            ("Lvl 30 Ahura", 30, En, "https://pbs.twimg.com/media/Cs1w7-QUIAApnHh.jpg"),
            ("Lvl 40 Ahura", 40, En, "https://pbs.twimg.com/media/Cs1w9nhVIAABgqg.jpg"),
            ("Lvl 50 Grendel", 50, En, "https://pbs.twimg.com/media/Cs1w-68UEAEjhEV.jpg"),
            ("Lvl 60 Grendel", 60, En, "https://pbs.twimg.com/media/Cs1xAKcVIAEXC9a.jpg"),

            ("Lvl 40 Ogler", 40, En, "https://pbs.twimg.com/media/Cn7oqbPUkAEimW2.jpg"),
            ("Lvl 40 Yog-Sothoth", 40, En, "https://pbs.twimg.com/media/Cqlr0pRVUAQxZ5g.jpg"),
            ("Lvl 60 Mahisha", 60, En, "https://pbs.twimg.com/media/Cqlr51CUAAA9lp3.jpg"),
            ("Lvl 60 Gugalanna", 60, En, "https://pbs.twimg.com/media/Cn7o6ppVUAArahp.jpg"),
            ("Lvl 75 Supergigante", 75, En, "https://pbs.twimg.com/media/CqlsAzGUkAAAcRT.jpg"),
            ("Lvl 75 Viridian Horn", 75, En, "https://pbs.twimg.com/media/Cn7o85cUEAAnlh0.jpg"),

            ("Lvl 50 Celeste", 50, En, "https://pbs.twimg.com/media/CfqXLhHUEAAF92L.jpg"),
            ("Lvl 50 Leviathan", 50, En, "https://pbs.twimg.com/media/CfqW-MVUIAEJrDP.jpg"),
            ("Lvl 50 Tiamat Omega", 50, En, "https://pbs.twimg.com/media/CfqXQA3UMAEMV7O.jpg"),
            ("Lvl 50 Tiamat", 50, En, "https://pbs.twimg.com/media/CfqWy2tUUAAr9yu.jpg"),
            ("Lvl 50 Veselago", 50, En, "https://pbs.twimg.com/media/Crtpu8RVMAALBKk.jpg"),
            ("Lvl 50 Yggdrasil", 50, En, "https://pbs.twimg.com/media/CfqXDAQVIAAixiA.jpg"),
            ("Lvl 60 Leviathan Omega", 60, En, "https://pbs.twimg.com/media/CfqXTAQUAAAu3ox.jpg"),
            ("Lvl 60 Yggdrasil Omega", 60, En, "https://pbs.twimg.com/media/CfuZgxLUkAArdGe.jpg"),
            ("Lvl 70 Colossus Omega", 70, En, "https://pbs.twimg.com/media/CfqXRjsUAAAwXTP.jpg"),
            ("Lvl 75 Celeste Omega", 75, En, "https://pbs.twimg.com/media/CfqXXVDUUAA0hAS.jpg"),
            ("Lvl 75 Luminiera Omega", 75, En, "https://pbs.twimg.com/media/CfqXWAhUsAAprzd.jpg"),
            ("Lvl 100 Apollo", 100, En, "https://pbs.twimg.com/media/CfqXI0lVAAAQgcj.jpg"),
            ("Lvl 100 Athena", 100, En, "https://pbs.twimg.com/media/Cg7oBQ_UYAEAIK7.jpg"),
            ("Lvl 100 Baal", 100, En, "https://pbs.twimg.com/media/CjLxhwmUoAEglVC.jpg"),
            ("Lvl 100 Celeste Omega", 100, En, "https://pbs.twimg.com/media/CfqZzDsUUAA5DEX.jpg"),
            ("Lvl 100 Colossus Omega", 100, En, "https://pbs.twimg.com/media/CfqZOt6VIAAniVV.jpg"),
            ("Lvl 100 Dark Angel Olivia", 100, En, "https://pbs.twimg.com/media/CfqXOjEUMAAXuK2.jpg"),
            ("Lvl 100 Garuda", 100, En, "https://pbs.twimg.com/media/CkVbjdpUYAAmnvb.jpg"),
            ("Lvl 100 Grand Order", 100, En, "https://pbs.twimg.com/media/CfqaAYfUUAQqgpv.jpg"),
            ("Lvl 100 Grani", 100, En, "https://pbs.twimg.com/media/CqwDYIbVMAE0VUR.jpg"),
            ("Lvl 100 Leviathan Omega", 100, En, "https://pbs.twimg.com/media/CfqZfExVIAA4YgF.jpg"),
            ("Lvl 100 Lich", 100, En, "https://pbs.twimg.com/media/CqwDaPAVYAQAYzq.jpg"),
            ("Lvl 100 Luminiera Omega", 100, En, "https://pbs.twimg.com/media/CfqZvtlVIAAgBeF.jpg"),
            ("Lvl 100 Macula Marius", 100, En, "https://pbs.twimg.com/media/CfqXAC1UIAAeGl-.jpg"),
            ("Lvl 100 Medusa", 100, En, "https://pbs.twimg.com/media/CfqXEh_UsAEb9dw.jpg"),
            ("Lvl 100 Nezha", 100, En, "https://pbs.twimg.com/media/CfqW0bzUMAAOJsU.jpg"),
            ("Lvl 100 Odin", 100, En, "https://pbs.twimg.com/media/CqwDWGjUIAEeJ4s.jpg"),
            ("Lvl 100 Proto Bahamut", 100, En, "https://pbs.twimg.com/media/CfqXYlBUAAQ1mtV.jpg"),
            ("Lvl 100 Tiamat Omega Ayr", 100, En, "https://pbs.twimg.com/media/CfqW2SWUEAAMr7S.jpg"),
            ("Lvl 100 Twin Elements", 100, En, "https://pbs.twimg.com/media/CfqXaAvUIAEUC8B.jpg"),
            ("Lvl 100 Yggdrasil Omega", 100, En, "https://pbs.twimg.com/media/Cfv1i6wUsAAZajc.jpg"),
            ("Lvl 120 Apollo", 120, En, "https://pbs.twimg.com/media/CfqZxihUEAEzcG-.jpg"),
            ("Lvl 120 Dark Angel Olivia", 120, En, "https://pbs.twimg.com/media/CfqZ3BwVIAEtIgy.jpg"),
            ("Lvl 120 Macula Marius", 120, En, "https://pbs.twimg.com/media/CfqZhE0UsAA_JqS.jpg"),
            ("Lvl 120 Medusa", 120, En, "https://pbs.twimg.com/media/CfqZlIcVIAAp8e_.jpg"),
            ("Lvl 120 Nezha", 120, En, "https://pbs.twimg.com/media/CfqW4BYUEAAeYSR.jpg"),
            ("Lvl 120 Twin Elements", 120, En, "https://pbs.twimg.com/media/CfqZQ_pUEAAQFI4.jpg"),
            ("Lvl 150 Proto Bahamut", 150, En, "https://pbs.twimg.com/media/CfqZ-YtVAAAt5qd.jpg"),
        ];

        let futures = bosses.iter().map(move |(name, level, language, url)| {
            let hasher = hasher.clone();
            async move {
                let hash = hasher.hash(&format!("{}:large", url)).await?;
                eprintln!("{} -> {:?}", name, hash);
                let result: anyhow::Result<Item> = Ok(Item {
                    name,
                    level: *level,
                    language: *language,
                    hash,
                });
                result
            }
        });

        use futures::stream::StreamExt;
        use futures::stream::TryStreamExt;
        let results: Vec<Item> = futures::stream::iter(futures)
            .buffer_unordered(bosses.len())
            .try_collect()
            .await?;

        use itertools::Itertools;
        let equivalent_bosses = results
            .iter()
            .tuple_combinations()
            .filter_map(|(a, b)| {
                if a.language != b.language && a.level == b.level && a.hash == b.hash {
                    Some((a.name, b.name))
                } else {
                    None
                }
            })
            .collect::<HashMap<&str, &str>>();

        let expected = maplit::hashmap! {
            "Lv30 アーフラー" => "Lvl 30 Ahura",
            "Lv40 アーフラー" => "Lvl 40 Ahura",
            "Lv50 ベオウルフ" => "Lvl 50 Grendel",
            "Lv60 ベオウルフ" => "Lvl 60 Grendel",

            "Lv40 ゲイザー" => "Lvl 40 Ogler",
            "Lv40 ヨグ＝ソトース" => "Lvl 40 Yog-Sothoth",
            "Lv60 グガランナ" => "Lvl 60 Gugalanna",
            "Lv60 マヒシャ" => "Lvl 60 Mahisha",
            "Lv75 スーペルヒガンテ" => "Lvl 75 Supergigante",
            "Lv75 エメラルドホーン" => "Lvl 75 Viridian Horn",

            "Lv50 セレスト" => "Lvl 50 Celeste",
            "Lv50 ティアマト" => "Lvl 50 Tiamat",
            "Lv50 ティアマト・マグナ" => "Lvl 50 Tiamat Omega",
            "Lv50 ユグドラシル" => "Lvl 50 Yggdrasil",
            "Lv50 リヴァイアサン" => "Lvl 50 Leviathan",
            "Lv50 ヴェセラゴ" => "Lvl 50 Veselago",
            "Lv60 ユグドラシル・マグナ" => "Lvl 60 Yggdrasil Omega",
            "Lv60 リヴァイアサン・マグナ" => "Lvl 60 Leviathan Omega",
            "Lv70 コロッサス・マグナ" => "Lvl 70 Colossus Omega",
            "Lv75 シュヴァリエ・マグナ" => "Lvl 75 Luminiera Omega",
            "Lv75 セレスト・マグナ" => "Lvl 75 Celeste Omega",
            "Lv100 Dエンジェル・オリヴィエ" => "Lvl 100 Dark Angel Olivia",
            "Lv100 アテナ" => "Lvl 100 Athena",
            "Lv100 アポロン" => "Lvl 100 Apollo",
            "Lv100 オーディン" => "Lvl 100 Odin",
            "Lv100 ガルーダ" => "Lvl 100 Garuda",
            "Lv100 グラニ" => "Lvl 100 Grani",
            "Lv100 コロッサス・マグナ" => "Lvl 100 Colossus Omega",
            "Lv100 シュヴァリエ・マグナ" => "Lvl 100 Luminiera Omega",
            "Lv100 ジ・オーダー・グランデ" => "Lvl 100 Grand Order",
            "Lv100 セレスト・マグナ" => "Lvl 100 Celeste Omega",
            "Lv100 ティアマト・マグナ＝エア" => "Lvl 100 Tiamat Omega Ayr",
            "Lv100 ナタク" => "Lvl 100 Nezha",
            "Lv100 バアル" => "Lvl 100 Baal",
            "Lv100 フラム＝グラス" => "Lvl 100 Twin Elements",
            "Lv100 プロトバハムート" => "Lvl 100 Proto Bahamut",
            "Lv100 マキュラ・マリウス" => "Lvl 100 Macula Marius",
            "Lv100 メドゥーサ" => "Lvl 100 Medusa",
            "Lv100 ユグドラシル・マグナ" => "Lvl 100 Yggdrasil Omega",
            "Lv100 リッチ" => "Lvl 100 Lich",
            "Lv100 リヴァイアサン・マグナ" => "Lvl 100 Leviathan Omega",
            "Lv120 Dエンジェル・オリヴィエ" => "Lvl 120 Dark Angel Olivia",
            "Lv120 アポロン" => "Lvl 120 Apollo",
            "Lv120 ナタク" => "Lvl 120 Nezha",
            "Lv120 フラム＝グラス" => "Lvl 120 Twin Elements",
            "Lv120 マキュラ・マリウス" => "Lvl 120 Macula Marius",
            "Lv150 プロトバハムート" => "Lvl 150 Proto Bahamut",
        };

        assert_eq!(equivalent_bosses, expected);

        Ok(())
    }
}
