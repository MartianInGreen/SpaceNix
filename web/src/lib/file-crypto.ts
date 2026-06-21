const MAGIC = new TextEncoder().encode("SPNXENC1");
const IV_BYTES = 12;
const PBKDF2_ITERATIONS = 310_000;

const encoder = new TextEncoder();

export async function deriveFileEncryptionKey(
  password: string,
  userIdentityHex: string
): Promise<CryptoKey> {
  if (!userIdentityHex) {
    throw new Error("Cannot derive file encryption key without a user identity.");
  }

  const passwordKey = await crypto.subtle.importKey(
    "raw",
    encoder.encode(password),
    "PBKDF2",
    false,
    ["deriveKey"]
  );

  return crypto.subtle.deriveKey(
    {
      name: "PBKDF2",
      salt: encoder.encode(`spacenix:file-content:v1:${userIdentityHex}`),
      iterations: PBKDF2_ITERATIONS,
      hash: "SHA-256",
    },
    passwordKey,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"]
  );
}

export async function encryptFileContent(
  key: CryptoKey,
  plaintext: ArrayBuffer
): Promise<ArrayBuffer> {
  const iv = crypto.getRandomValues(new Uint8Array(IV_BYTES));
  const ciphertext = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, plaintext);
  const output = new Uint8Array(MAGIC.length + iv.length + ciphertext.byteLength);

  output.set(MAGIC, 0);
  output.set(iv, MAGIC.length);
  output.set(new Uint8Array(ciphertext), MAGIC.length + iv.length);

  return output.buffer;
}

export async function decryptFileContent(
  key: CryptoKey,
  content: ArrayBuffer
): Promise<ArrayBuffer> {
  const bytes = new Uint8Array(content);
  if (!hasMagic(bytes)) {
    return content.slice(0);
  }
  if (bytes.length <= MAGIC.length + IV_BYTES) {
    throw new Error("Encrypted file content is truncated.");
  }

  const iv = bytes.slice(MAGIC.length, MAGIC.length + IV_BYTES);
  const ciphertext = bytes.slice(MAGIC.length + IV_BYTES);

  return crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, ciphertext);
}

function hasMagic(bytes: Uint8Array): boolean {
  if (bytes.length < MAGIC.length) return false;
  return MAGIC.every((byte, index) => bytes[index] === byte);
}
