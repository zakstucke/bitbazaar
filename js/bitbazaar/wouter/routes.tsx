import type React from "react";
import { Route, Switch } from "wouter";

import type { InnerRouteConfig_T, RouteConfig_T } from "./types";
import { ValiWrapper } from "./valiWrapper";

/** The producer of wouter <Switch /> components and all the routes, with a fallback and param coercion.
 * @param Comp404 The component to render when no routes match. This will be returned when a wouter path matched but coercion failed. Also given as a final route to be used when no other routes match.
 * @param routes An object of route configs, key is the rout name, value is the route() fn output.
 *
 * @returns an object containing the wouter <Switch /> component and all the routes, should be wrapped in a <Router />.
 * Render with `routes.switch`, build paths with `routes.paths.home.build({foo: "bar"})`, etc.
 */
export const routes = <const T extends Record<string, InnerRouteConfig_T<any>>>({
    Comp404,
    routes,
}: {
    Comp404: React.ComponentType;
    routes: T;
}): {
    switch: JSX.Element;
    paths: {
        [K in keyof T]: T[K] extends InnerRouteConfig_T<infer X> ? RouteConfig_T<X> : never;
    };
} => {
    const output: Record<string, RouteConfig_T<any>> = {};
    Object.entries(routes).forEach(([key, props]) => {
        output[key] = {
            ...props,
            route: (
                <Route path={props.path} key={props.path}>
                    {(params) => (
                        <ValiWrapper
                            params={params}
                            validators={props.validators}
                            Comp={props.Comp}
                            Comp404={Comp404}
                        />
                    )}
                </Route>
            ),
        };
    });

    // eslint-disable-next-line @typescript-eslint/no-unsafe-return
    const allRoutes = Object.values(output).map(({ route }) => route);

    return {
        switch: (
            <Switch>
                {allRoutes}
                {/* eslint-disable-next-line @typescript-eslint/no-unsafe-assignment */}
                <Route component={Comp404 as any} />
            </Switch>
        ),
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        paths: output as any,
    };
};
