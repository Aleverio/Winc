import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import test from "node:test";

import { channelBase, channelOf, releaseAssetUrl, releaseConfig, updateFeedTag } from "./config.mjs";
import { publicKey } from "./update-signing.mjs";

test("stable updates use a fixed GitHub release feed", () => {
  assert.equal(channelOf("1.2.3"), "stable");
  assert.equal(updateFeedTag("1.2.3"), "winc-update-feed-stable");
  assert.equal(
    channelBase("1.2.3", "Aleverio/Winc"),
    "https://github.com/Aleverio/Winc/releases/download/winc-update-feed-stable",
  );
});

test("prerelease updates use a separate fixed feed", () => {
  assert.equal(channelOf("1.2.3-beta.1"), "beta");
  assert.equal(updateFeedTag("1.2.3-beta.1"), "winc-update-feed-beta");
  assert.equal(
    channelBase("1.2.3-beta.1", "Aleverio/Winc"),
    "https://github.com/Aleverio/Winc/releases/download/winc-update-feed-beta",
  );
});

test("installer URLs point to immutable versioned release assets", () => {
  assert.equal(
    releaseAssetUrl("1.2.3", "Winc-1.2.3-windows-x64-setup.exe", "Aleverio/Winc"),
    "https://github.com/Aleverio/Winc/releases/download/v1.2.3/Winc-1.2.3-windows-x64-setup.exe",
  );
});

test("release configuration embeds the channel URL and matching public key", () => {
  const { privateKey } = generateKeyPairSync("ed25519");
  const config = releaseConfig({
    version: "2.0.0-beta.1",
    key: privateKey,
    repository: "Aleverio/Winc",
  });
  assert.equal(
    config.updateChannels,
    "beta=https://github.com/Aleverio/Winc/releases/download/winc-update-feed-beta",
  );
  assert.equal(config.updatePublicKey, publicKey(privateKey));
});

test("invalid repository names are rejected", () => {
  assert.throws(() => channelBase("1.2.3", "not/a/repository/path"), /owner\/repository/);
});
