import { ProxyOptions, UserConfig } from "vite";

import { genPath } from "../utils/genPath";

export interface ProxyConf {
    // Where matches should be sent to:
    target: string;
    // The paths to match on (e.g. /api/):
    matches: string[];
    // If set to true, everything BUT these paths will be forwarded to the target:
    // Useful e.g. when only a few paths should go through vite.
    negate?: boolean;
}

type ViteProxy_T = Exclude<Exclude<UserConfig["server"], undefined>["proxy"], undefined>;

const genTargetOptions = (target: string): ProxyOptions => {
    return {
        // Make sure DOES NOT end with /: (if it has one it won't forward sub paths)
        target: genPath(target, { eSlash: false }),
        // If true, django csrf breaks I think because csrf comes in as port 3000,
        // but vites rewritten headers as django's port 8000,
        // seems like there aren't any issues in dev with just leaving as vite domain.
        changeOrigin: false,
    };
};

export const genBackendProxies = ({ target, matches, negate }: ProxyConf): ViteProxy_T => {
    const result: ViteProxy_T = {};
    if (negate) {
        // Create a regex that matches everything BUT the paths, vite recognises a regex when it starts with a ^:
        result[`^(?!${matches.join("|")}).*$`] = genTargetOptions(target);
    } else {
        matches.forEach((path) => {
            result[path] = genTargetOptions(target);
        });
    }
    return result;
};
