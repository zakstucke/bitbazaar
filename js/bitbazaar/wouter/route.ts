import type React from "react";

import { assertNever, genPath } from "@root/utils";

import type { InnerRouteConfig_T, RouteParamsOutput_T, RouteParams_T } from "./types";

/** A path config holder.
 *
 * @param base The base path, e.g. "/user/".
 * @param params An array of [param_name, param_schema] tuples, e.g. [["user_id", number()]]. Where the second argument is "string"/"float"/"int"/"boolean". Coercion and type hinting for the params is handled internally.
 * @param Component The component to render when the path is matched.
 */
export const route = <const Params extends RouteParams_T>({
    base,
    params,
    Comp,
}: {
    base: string;
    params: Params;
    Comp: React.ComponentType<RouteParamsOutput_T<Params>>;
}): InnerRouteConfig_T<RouteParamsOutput_T<Params>> => {
    // Make sure the base path starts and ends with a slash:
    base = genPath(base, { sShlash: true, eSlash: true });

    // Produce the path by adding :$param to the base path for each param:
    let path = base;
    params.forEach(([param_name]) => {
        path += `:${param_name}/`;
    });

    // Remove the final slash if not home:
    if (path !== "/") {
        path = path.slice(0, -1);
    }

    const validators: Record<string, (param: string) => unknown> = {};
    params.forEach(([param_name, param_type]) => {
        // Create the validator depending on the type:
        let validator: ((param: string) => unknown) | null = null;
        switch (param_type) {
            case "float": {
                validator = (param: string) => {
                    // Try and convert to a number:
                    const num = parseFloat(param);
                    if (Number.isNaN(num)) {
                        throw new Error("Invalid number");
                    }
                    return num;
                };
                break;
            }
            case "int": {
                validator = (param: string) => {
                    // Try and convert to a number:
                    const num = parseInt(param);
                    if (Number.isNaN(num)) {
                        throw new Error("Invalid number");
                    }
                    return num;
                };
                break;
            }
            case "boolean": {
                validator = (param: string) => {
                    const lower = param.toLowerCase();
                    if (["true", "True", "1", "y"].includes(lower)) {
                        return true;
                    }
                    if (["false", "False", "0", "n"].includes(lower)) {
                        return false;
                    }
                    throw new Error("Invalid boolean");
                };
                break;
            }
            case "string": {
                validator = (param: string) => param;
                break;
            }
            default:
                assertNever(param_type);
        }
        validators[param_name] = validator as (param: string) => unknown;
    });

    const routeObj: InnerRouteConfig_T<RouteParamsOutput_T<Params>> = {
        build: (buildParams: RouteParamsOutput_T<Params>) => {
            let builtPath = path;
            params.forEach(([param_name]) => {
                builtPath = builtPath.replace(`:${param_name}`, buildParams[param_name] as string);
            });
            return builtPath;
        },
        validators,
        Comp,
        path,
    };

    return routeObj;
};
