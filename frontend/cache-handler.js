const fs = require('fs');
const path = require('path');

const cacheDir = process.env.NEXT_CACHE_DIR || path.join(process.cwd(), '.next', 'cache');

function getCachePath(key) {
  const safe = key.replace(/[^a-zA-Z0-9_-]/g, '_');
  return path.join(cacheDir, `${safe}.json`);
}

function ensureDir() {
  fs.mkdirSync(cacheDir, { recursive: true });
}

function prepareForSerialization(value) {
  if (value === null || value === undefined) return value;
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) {
    return { __type: 'Buffer', data: Buffer.from(value).toString('base64') };
  }
  if (value instanceof Map) {
    return {
      __type: 'Map',
      entries: Array.from(value.entries()).map(([k, v]) => [k, prepareForSerialization(v)]),
    };
  }
  if (Array.isArray(value)) {
    return value.map(prepareForSerialization);
  }
  if (typeof value === 'object') {
    const result = {};
    for (const key of Object.keys(value)) {
      result[key] = prepareForSerialization(value[key]);
    }
    return result;
  }
  return value;
}

function serialize(obj) {
  return JSON.stringify(prepareForSerialization(obj));
}

function deserialize(json) {
  return JSON.parse(json, (_key, value) => {
    if (value && typeof value === 'object') {
      if (value.__type === 'Map') {
        return new Map(
          value.entries.map(([k, v]) => [
            k,
            v && v.__type === 'Buffer' ? Buffer.from(v.data, 'base64') : v,
          ]),
        );
      }
      if (value.__type === 'Buffer') {
        return Buffer.from(value.data, 'base64');
      }
      if (value.type === 'Buffer' && Array.isArray(value.data)) {
        return Buffer.from(value.data);
      }
    }
    return value;
  });
}

module.exports = class CacheHandler {
  constructor(options) {
    this.options = options;
    ensureDir();
  }

  async get(key) {
    const filePath = getCachePath(key);
    try {
      const raw = fs.readFileSync(filePath, 'utf8');
      return deserialize(raw);
    } catch {
      return null;
    }
  }

  async set(key, data, ctx) {
    const filePath = getCachePath(key);
    ensureDir();
    fs.writeFileSync(
      filePath,
      serialize({ value: data, lastModified: Date.now(), tags: ctx.tags }),
    );
  }

  async revalidateTag(tags) {
    tags = [tags].flat();
    try {
      const files = fs.readdirSync(cacheDir);
      for (const file of files) {
        if (!file.endsWith('.json')) continue;
        const filePath = path.join(cacheDir, file);
        try {
          const raw = fs.readFileSync(filePath, 'utf8');
          const entry = deserialize(raw);
          if (entry.tags && entry.tags.some((t) => tags.includes(t))) {
            fs.unlinkSync(filePath);
          }
        } catch {
          // skip corrupt entries
        }
      }
    } catch {
      // cache dir may not exist yet
    }
  }

  resetRequestCache() {}
};
