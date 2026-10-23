import * as THREE from "three";

/**
 * Contact pools: the ground darkens softly under the wheel and under the rider's body, so they
 * sit on the street instead of floating. Analytic, in the ground's shader, from two points the
 * physics poses give each frame (after EUC Thrills' ultra ground contact, src/render/ultra/
 * groundContact.ts, MIT; our own code). Strength 0 turns a pool off.
 */
export const contactAo = {
  /** Wheel contact: scene x, scene z, radius (m), strength (0-1). */
  wheel: { value: new THREE.Vector4(0, 0, 0.6, 0) },
  /** Rider's body: scene x, scene z, radius (m), strength (0-1); stretched along `axis`. */
  body: { value: new THREE.Vector4(0, 0, 1.05, 0) },
  /** The sun's shadow direction on the ground (unit, scene x and z), and the stretch along it. */
  axis: { value: new THREE.Vector3(1, 0, 1.6) },
};

const VERTEX_HEAD = "varying vec3 vContactWorld;\n";
const FRAGMENT_HEAD = `varying vec3 vContactWorld;
uniform vec4 contactWheel;
uniform vec4 contactBody;
uniform vec3 contactAxis;
float contactPool(vec2 d, vec4 pool) {
  float r = length(d) / max(pool.z, 1e-3);
  float k = 1.0 - smoothstep(0.0, 1.0, r);
  return pool.w * k * k;
}
`;

/** Adds the contact pools to a ground material (its clones need it again: three.js does not copy it). */
export function withContactAo<T extends THREE.Material>(material: T): T {
  material.onBeforeCompile = (shader) => {
    shader.uniforms.contactWheel = contactAo.wheel;
    shader.uniforms.contactBody = contactAo.body;
    shader.uniforms.contactAxis = contactAo.axis;
    shader.vertexShader =
      VERTEX_HEAD +
      shader.vertexShader.replace(
        "#include <project_vertex>",
        "#include <project_vertex>\n  vContactWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;",
      );
    shader.fragmentShader =
      FRAGMENT_HEAD +
      shader.fragmentShader.replace(
        "#include <opaque_fragment>",
        `{
    vec2 p = vContactWorld.xz;
    float wheel = contactPool(p - contactWheel.xy, contactWheel);
    vec2 d = p - contactBody.xy;
    vec2 along = contactAxis.xy;
    vec2 local = vec2(dot(d, along) / contactAxis.z, dot(d, vec2(-along.y, along.x)));
    float body = contactPool(local, contactBody);
    outgoingLight *= (1.0 - wheel) * (1.0 - body);
  }
  #include <opaque_fragment>`,
      );
  };
  material.customProgramCacheKey = () => "contact-ao-1";
  return material;
}
