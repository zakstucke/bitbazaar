import { RouteParams_T } from "./types";

/** Simple type wrapper fn to make writing a params definition shorter.
 * @param params An array of tuples, first item is the param name, second is the param type as a string.
 *
 * Without this wrapper fn, to get correct type, `routeParams([["user_id", number()]])` would need to be written as:
 * `[["user_id", number()]] as const satisfies RouteParams_T`
 */
export const routeParams = <const T extends RouteParams_T>(params: T): T => {
    return params;
};
