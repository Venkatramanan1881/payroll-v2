// Dev only: prints the current 6-digit authenticator code for a dev login kept in backend/.env.
//   node web/scripts/dev-totp.js            -> DEV_ADMIN_TOTP_SECRET
//   node web/scripts/dev-totp.js VENDOR     -> DEV_VENDOR_TOTP_SECRET
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

const who = (process.argv[2] || "ADMIN").toUpperCase();
const env = fs.readFileSync(path.join(__dirname, "../../backend/.env"), "utf8");
const secret = env.match(new RegExp(`^DEV_${who}_TOTP_SECRET=(.+)$`, "m"))?.[1]?.trim();
if (!secret) throw new Error(`DEV_${who}_TOTP_SECRET is not in backend/.env`);

// RFC 6238: HMAC-SHA1 over the 30-second counter, base32 key.
const A = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
let bits = "";
for (const ch of secret.replace(/=+$/, "").toUpperCase()) bits += A.indexOf(ch).toString(2).padStart(5, "0");
const key = Buffer.from(bits.match(/.{8}/g).map((b) => parseInt(b, 2)));
const counter = Buffer.alloc(8);
counter.writeBigUInt64BE(BigInt(Math.floor(Date.now() / 30000)));
const h = crypto.createHmac("sha1", key).update(counter).digest();
const o = h[19] & 15;
const code = String((h.readUInt32BE(o) & 0x7fffffff) % 1e6).padStart(6, "0");
console.log(`${code}  (valid for ${30 - (Math.floor(Date.now() / 1000) % 30)} more seconds)`);
