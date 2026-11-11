# 3D headphones

`headphones.src.js` builds the headphones in the download section from simple shapes
with [three.js](https://threejs.org) (MIT). It's bundled into `../headphones.js`:

```
npm install three esbuild
npx esbuild 3d/headphones.src.js --bundle --format=esm --minify --target=es2020 --legal-comments=eof --outfile=headphones.js
```

The page loads it only when the download section comes near.
